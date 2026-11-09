use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

include!(concat!(env!("OUT_DIR"), "/webui.rs"));

const NOT_BUILT: &str = "<!doctype html><meta charset=utf-8><title>bawkbit</title>\
<body style=\"font:14px ui-monospace,monospace;background:#13100f;color:#c3bcbb;padding:40px\">\
The web UI is not built into this copy of bawkbit. Run <code>bun install</code> and \
<code>bun run build</code> in <code>web/</code>, then build bawkbit again.";

fn find(path: &str) -> Option<&'static [u8]> {
    FILES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

/// Serves a built file, or the app shell for any path the app routes itself.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(bytes) = (!path.is_empty()).then(|| find(path)).flatten() {
        let immutable = path.starts_with("assets/") || path.starts_with("flags/");
        return file(path, bytes, immutable);
    }
    if ["api/", "assets/", "flags/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    match find("index.html") {
        Some(bytes) => file("index.html", bytes, false),
        None => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            NOT_BUILT,
        )
            .into_response(),
    }
}

fn file(path: &str, bytes: &'static [u8], immutable: bool) -> Response {
    let cache = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime(path)));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "webmanifest" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
