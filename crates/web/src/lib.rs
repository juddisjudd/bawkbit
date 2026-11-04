mod assets;
mod auth;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bawkbit_core::settings::{Indexer, Settings};
use bawkbit_core::{
    Action, AddRequest, Backup, Core, Details, Preview, Reply, Rss, RssAction, Rule, RuleMatch,
    SearchReply, SearchRequest, Snapshot, Source,
};
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tower_http::compression::CompressionLayer;

pub use auth::{generate_password, hash_password};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Off,
    Listening(SocketAddr),
    Failed(String),
}

/// The web UI server, kept in step with the web settings: it starts, stops and moves port as they change.
#[derive(Clone)]
pub struct WebUi {
    status: Arc<Mutex<Status>>,
}

impl WebUi {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}

/// Starts the server supervisor on the current tokio runtime.
pub fn spawn(core: Core) -> WebUi {
    let status = Arc::new(Mutex::new(Status::Off));
    let ui = WebUi {
        status: status.clone(),
    };
    let auth = Arc::new(auth::Auth::default());
    tokio::spawn(async move {
        let mut running: Option<(String, u16, watch::Sender<bool>)> = None;
        let mut password = String::new();
        let mut failed_at: Option<(String, u16)> = None;
        loop {
            let web = core.settings().web;
            if web.password_hash != password {
                if !password.is_empty() {
                    auth.clear();
                }
                password = web.password_hash.clone();
            }
            let want = web
                .enabled
                .then(|| (web.address.trim().to_string(), web.port));
            let current = running
                .as_ref()
                .map(|(address, port, _)| (address.clone(), *port));
            if current != want {
                if let Some((_, _, stop)) = running.take() {
                    let _ = stop.send(true);
                    *status.lock().unwrap() = Status::Off;
                }
                if let Some((address, port)) =
                    want.clone().filter(|key| failed_at.as_ref() != Some(key))
                {
                    match bind(&address, port).await {
                        Ok(listener) => {
                            let local = listener.local_addr().ok();
                            let (stop, stopped) = watch::channel(false);
                            let state = AppState {
                                core: core.clone(),
                                auth: auth.clone(),
                                stop: stopped.clone(),
                            };
                            tokio::spawn(serve(listener, router(state), stopped));
                            if let Some(local) = local {
                                tracing::info!("web UI on http://{local}");
                                *status.lock().unwrap() = Status::Listening(local);
                            }
                            running = Some((address, port, stop));
                            failed_at = None;
                        }
                        Err(err) => {
                            tracing::warn!("{err}");
                            *status.lock().unwrap() = Status::Failed(err);
                            failed_at = Some((address, port));
                        }
                    }
                }
                if want.is_none() {
                    failed_at = None;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    ui
}

async fn bind(address: &str, port: u16) -> Result<tokio::net::TcpListener, String> {
    let address = if address.is_empty() {
        "0.0.0.0"
    } else {
        address
    };
    let target = if address.contains(':') && !address.starts_with('[') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    };
    tokio::net::TcpListener::bind(&target)
        .await
        .map_err(|err| format!("cannot open the web UI on {target}: {err}"))
}

async fn serve(listener: tokio::net::TcpListener, app: Router, mut stopped: watch::Receiver<bool>) {
    let service = app.into_make_service_with_connect_info::<SocketAddr>();
    let shutdown = async move {
        let _ = stopped.wait_for(|stop| *stop).await;
    };
    if let Err(err) = axum::serve(listener, service)
        .with_graceful_shutdown(shutdown)
        .await
    {
        tracing::warn!("web UI stopped: {err}");
    }
}

#[derive(Clone)]
struct AppState {
    core: Core,
    auth: Arc<auth::Auth>,
    stop: watch::Receiver<bool>,
}

fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/snapshot", get(snapshot))
        .route("/events", get(events))
        .route("/torrents", post(add))
        .route("/torrents/{id}", get(details))
        .route("/preview", post(preview))
        .route("/actions", post(action))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/search", post(search))
        .route("/indexers/test", post(test_indexer))
        .route("/rss", get(get_rss).post(rss_action))
        .route("/rss/matches", post(rule_matches))
        .route("/backup", get(get_backup).post(restore))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_login));
    let open = Router::new()
        .route("/session", get(session))
        .route("/login", post(login))
        .route("/logout", post(logout));
    Router::new()
        .nest("/api", protected.merge(open))
        .fallback(assets::serve)
        .layer(middleware::from_fn(guard))
        .layer(CompressionLayer::new())
        .with_state(state)
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Body {
            error: String,
        }
        (self.0, Json(Body { error: self.1 })).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError(StatusCode::BAD_REQUEST, format!("{err:#}"))
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

fn peer(request: &Request) -> Option<SocketAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0)
}

fn signed_in(state: &AppState, addr: Option<SocketAddr>, headers: &HeaderMap) -> bool {
    let web = state.core.settings().web;
    if web.local_bypass && auth::is_local(addr, headers) {
        return true;
    }
    auth::cookie_token(headers).is_some_and(|token| state.auth.check(&token))
}

async fn require_login(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if signed_in(&state, peer(&request), request.headers()) {
        next.run(request).await
    } else {
        ApiError(StatusCode::UNAUTHORIZED, "log in first".into()).into_response()
    }
}

/// Rejects writes from other sites and sets headers that keep the UI out of frames.
async fn guard(request: Request, next: Next) -> Response {
    let writes = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if writes && !same_origin(request.headers()) {
        return ApiError(StatusCode::FORBIDDEN, "cross-site request refused".into())
            .into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'",
        ),
    );
    response
}

fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let origin_host = origin.split_once("://").map(|(_, rest)| rest);
    origin_host.is_some() && origin_host == host
}

#[derive(Serialize)]
struct SessionInfo {
    signed_in: bool,
    has_password: bool,
    version: &'static str,
}

async fn session(State(state): State<AppState>, request: Request) -> Json<SessionInfo> {
    Json(SessionInfo {
        signed_in: signed_in(&state, peer(&request), request.headers()),
        has_password: !state.core.settings().web.password_hash.is_empty(),
        version: bawkbit_core::VERSION,
    })
}

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<Login>,
) -> Response {
    let web = state.core.settings().web;
    match state
        .auth
        .login(addr.ip(), &web, &body.username, &body.password)
    {
        Ok(token) => (
            [(header::SET_COOKIE, auth::session_cookie(&token))],
            Json(serde_json::json!({ "ok": true })),
        )
            .into_response(),
        Err(auth::LoginError::Blocked(seconds)) => ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            format!("too many wrong tries. try again in {seconds} seconds."),
        )
        .into_response(),
        Err(auth::LoginError::NoPassword) => ApiError(
            StatusCode::FORBIDDEN,
            "set a web UI password in bawkbit's settings first.".into(),
        )
        .into_response(),
        Err(auth::LoginError::Wrong) => {
            ApiError(StatusCode::UNAUTHORIZED, "wrong name or password.".into()).into_response()
        }
    }
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = auth::cookie_token(&headers) {
        state.auth.logout(&token);
    }
    (
        [(header::SET_COOKIE, auth::expired_cookie())],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

async fn snapshot(State(state): State<AppState>) -> Json<Arc<Snapshot>> {
    Json(state.core.snapshot())
}

/// One `snapshot` event a second, and a `finished` event each time a torrent finishes, for as long as the page is open.
async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let updates = state.core.subscribe();
    let finished = state.core.subscribe_finished();
    let stop = state.stop.clone();
    let stream = futures_util::stream::unfold(
        (updates, finished, stop, true),
        |(mut updates, mut finished, mut stop, first)| async move {
            let event = if first {
                None
            } else {
                loop {
                    tokio::select! {
                        changed = updates.changed() => {
                            changed.ok()?;
                            break None;
                        }
                        done = finished.recv() => match done {
                            Ok(done) => break Event::default().event("finished").json_data(&done).ok(),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                        },
                        _ = stop.wait_for(|stop| *stop) => return None,
                    }
                }
            };
            let event = match event {
                Some(event) => event,
                None => {
                    let snapshot = updates.borrow_and_update().clone();
                    Event::default()
                        .event("snapshot")
                        .json_data(&*snapshot)
                        .ok()?
                }
            };
            Some((Ok(event), (updates, finished, stop, false)))
        },
    );
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn details(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Details> {
    state
        .core
        .details(&id)
        .map(Json)
        .map_err(|err| ApiError(StatusCode::NOT_FOUND, format!("{err:#}")))
}

#[derive(Serialize)]
struct Added {
    id: String,
}

async fn add(State(state): State<AppState>, Json(request): Json<AddRequest>) -> ApiResult<Added> {
    let id = state.core.add(request).await?;
    Ok(Json(Added { id }))
}

async fn preview(State(state): State<AppState>, Json(source): Json<Source>) -> ApiResult<Preview> {
    Ok(Json(state.core.preview(source).await?))
}

async fn action(State(state): State<AppState>, Json(action): Json<Action>) -> ApiResult<Reply> {
    Ok(Json(state.core.execute(action).await?))
}

async fn search(
    State(state): State<AppState>,
    Json(request): Json<SearchRequest>,
) -> ApiResult<SearchReply> {
    Ok(Json(state.core.search(request).await?))
}

#[derive(Serialize)]
struct Tested {
    message: String,
}

async fn test_indexer(
    State(state): State<AppState>,
    Json(indexer): Json<Indexer>,
) -> ApiResult<Tested> {
    let message = state.core.test_indexer(indexer).await?;
    Ok(Json(Tested { message }))
}

async fn get_rss(State(state): State<AppState>) -> Json<Rss> {
    Json(state.core.rss())
}

async fn rss_action(
    State(state): State<AppState>,
    Json(action): Json<RssAction>,
) -> ApiResult<Rss> {
    Ok(Json(state.core.rss_execute(action).await?))
}

async fn rule_matches(
    State(state): State<AppState>,
    Json(rule): Json<Rule>,
) -> ApiResult<Vec<RuleMatch>> {
    Ok(Json(state.core.rule_matches(&rule)?))
}

async fn get_backup(State(state): State<AppState>) -> Json<Backup> {
    Json(state.core.backup())
}

async fn restore(State(state): State<AppState>, Json(backup): Json<Backup>) -> ApiResult<Reply> {
    state.core.restore(backup).await?;
    Ok(Json(Reply::Done))
}

async fn get_settings(State(state): State<AppState>) -> Json<Settings> {
    Json(state.core.settings().public())
}

async fn put_settings(
    State(state): State<AppState>,
    Json(mut incoming): Json<Settings>,
) -> ApiResult<Settings> {
    if let Some(password) = incoming.web.password.take().filter(|p| !p.is_empty()) {
        if password.chars().count() < 8 {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "use at least 8 characters for the password.".into(),
            ));
        }
        state
            .core
            .set_web_password_hash(hash_password(&password)?)?;
    }
    Ok(Json(state.core.update_settings(incoming).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_origin_checks_the_host() {
        let mut headers = HeaderMap::new();
        assert!(same_origin(&headers));
        headers.insert(header::HOST, "box:8228".parse().unwrap());
        headers.insert(header::ORIGIN, "http://box:8228".parse().unwrap());
        assert!(same_origin(&headers));
        headers.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!same_origin(&headers));
    }
}
