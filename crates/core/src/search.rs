use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use reqwest::{Client, StatusCode, Url, header};
use serde_json::Value;

use crate::api::{SearchCategory, SearchFailure, SearchReply, SearchRequest, SearchResult};
use crate::settings::{Indexer, IndexerKind};

/// Prowlarr waits for its slowest indexer before it answers, so this is generous.
const TIMEOUT: Duration = Duration::from_secs(60);
const REDIRECTS: usize = 8;

/// Redirects are followed by hand: indexers send magnet links as redirects, which an HTTP client cannot follow.
pub fn client() -> Client {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(TIMEOUT)
        .user_agent(format!("bawkbit/{}", crate::VERSION))
        .build()
        .expect("the HTTP client builds")
}

pub async fn search(client: &Client, indexers: &[Indexer], request: &SearchRequest) -> SearchReply {
    let mut tasks = tokio::task::JoinSet::new();
    for indexer in indexers.iter().filter(|indexer| indexer.enabled).cloned() {
        let (client, request) = (client.clone(), request.clone());
        tasks.spawn(async move {
            let result = search_one(&client, &indexer, &request).await;
            (indexer, result)
        });
    }
    let mut results = Vec::new();
    let mut failures = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        let Ok((indexer, result)) = joined else {
            continue;
        };
        match result {
            Ok((found, failed)) => {
                results.extend(found);
                failures.extend(failed);
            }
            Err(err) => failures.push(SearchFailure {
                source: name(&indexer),
                error: format!("{err:#}"),
            }),
        }
    }
    failures.sort_by(|a, b| a.source.cmp(&b.source));
    SearchReply {
        results: merge(results),
        failures,
    }
}

async fn search_one(
    client: &Client,
    indexer: &Indexer,
    request: &SearchRequest,
) -> Result<(Vec<SearchResult>, Vec<SearchFailure>)> {
    let key = key(indexer)?;
    let via = name(indexer);
    match indexer.kind {
        IndexerKind::Prowlarr => {
            let url = prowlarr_search_url(&indexer.url, request)?;
            let value = get_json(client, url, Some(key)).await?;
            Ok((parse_prowlarr(&value, &via), Vec::new()))
        }
        IndexerKind::Jackett => {
            let url = jackett_search_url(&indexer.url, key, request)?;
            let value = get_json(client, url, None).await?;
            Ok(parse_jackett(&value, &via))
        }
    }
}

/// Checks the address and key, and says how many indexers stand behind them.
pub async fn test(client: &Client, indexer: &Indexer) -> Result<String> {
    let key = key(indexer)?;
    match indexer.kind {
        IndexerKind::Prowlarr => {
            let url = join(&base(&indexer.url)?, "api/v1/indexer");
            let value = get_json(client, url, Some(key)).await?;
            let count = value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| is_torrent(item))
                        .filter(|item| {
                            field(item, "enable")
                                .and_then(Value::as_bool)
                                .unwrap_or(true)
                        })
                        .count()
                })
                .unwrap_or(0);
            Ok(match count {
                0 => "connected, but no torrent indexers are turned on in prowlarr".into(),
                1 => "connected: 1 torrent indexer".into(),
                n => format!("connected: {n} torrent indexers"),
            })
        }
        IndexerKind::Jackett => {
            let mut url = join(
                &base(&indexer.url)?,
                "api/v2.0/indexers/all/results/torznab/api",
            );
            url.query_pairs_mut()
                .append_pair("apikey", key)
                .append_pair("t", "caps");
            let body = get_text(client, url, None).await?;
            if let Some(description) = torznab_error(&body) {
                bail!("{description}");
            }
            if !body.contains("<caps") {
                bail!("it did not answer like jackett; check the address");
            }
            Ok("connected".into())
        }
    }
}

pub enum Fetched {
    Torrent(Vec<u8>),
    Magnet(String),
}

/// Follows a download link to the .torrent file or the magnet link it ends in.
pub async fn fetch_link(client: &Client, link: &str) -> Result<Fetched> {
    let mut current = link.trim().to_string();
    for _ in 0..REDIRECTS {
        if current.to_ascii_lowercase().starts_with("magnet:") {
            return Ok(Fetched::Magnet(current));
        }
        let url = Url::parse(&current)
            .map_err(|_| anyhow!("{current} is not a link bawkbit can open"))?;
        let response = client.get(url.clone()).send().await.map_err(describe)?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| anyhow!("the link redirected nowhere"))?
                .trim()
                .to_string();
            current = if location.to_ascii_lowercase().starts_with("magnet:") {
                location
            } else {
                url.join(&location)
                    .map_err(|_| anyhow!("the link redirected to {location}"))?
                    .to_string()
            };
            continue;
        }
        if !status.is_success() {
            bail!("the link answered {status}");
        }
        let bytes = response.bytes().await.map_err(describe)?;
        if bytes.first() == Some(&b'd') {
            return Ok(Fetched::Torrent(bytes.to_vec()));
        }
        let text = String::from_utf8_lossy(&bytes);
        let text = text.trim();
        if text.to_ascii_lowercase().starts_with("magnet:") {
            return Ok(Fetched::Magnet(text.to_string()));
        }
        bail!("the link gave a web page, not a torrent");
    }
    bail!("the link redirected too many times")
}

fn key(indexer: &Indexer) -> Result<&str> {
    let key = indexer.api_key.trim();
    if key.is_empty() {
        bail!("add its API key in settings");
    }
    Ok(key)
}

fn name(indexer: &Indexer) -> String {
    let name = indexer.name.trim();
    if name.is_empty() {
        indexer.kind.label().into()
    } else {
        name.into()
    }
}

fn base(address: &str) -> Result<Url> {
    let trimmed = address.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        bail!("add its address in settings");
    }
    let full = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let mut url = Url::parse(&full).map_err(|_| anyhow!("{address} is not a web address"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("{address} is not a web address");
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn join(base: &Url, path: &str) -> Url {
    let mut url = base.clone();
    let full = format!("{}/{path}", base.path().trim_end_matches('/'));
    url.set_path(&full);
    url
}

pub fn prowlarr_search_url(address: &str, request: &SearchRequest) -> Result<Url> {
    let mut url = join(&base(address)?, "api/v1/search");
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("query", request.query.trim())
            .append_pair("type", "search")
            .append_pair("limit", "100");
        for category in request.category.newznab() {
            query.append_pair("categories", &category.to_string());
        }
    }
    Ok(url)
}

pub fn jackett_search_url(address: &str, key: &str, request: &SearchRequest) -> Result<Url> {
    let mut url = join(&base(address)?, "api/v2.0/indexers/all/results");
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("apikey", key)
            .append_pair("query", request.query.trim());
        let categories = request.category.newznab();
        if !categories.is_empty() {
            let list: Vec<String> = categories.iter().map(u32::to_string).collect();
            query.append_pair("Category[]", &list.join(","));
        }
    }
    Ok(url)
}

async fn get_text(client: &Client, url: Url, key: Option<&str>) -> Result<String> {
    let mut request = client.get(url);
    if let Some(key) = key {
        request = request.header("X-Api-Key", key);
    }
    let response = request.send().await.map_err(describe)?;
    let status = response.status();
    let body = response.text().await.map_err(describe)?;
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        bail!("the API key was refused");
    }
    if status.is_redirection() {
        bail!("it sent bawkbit to a login page; check the address and the API key");
    }
    if !status.is_success() {
        let message = torznab_error(&body).or_else(|| {
            serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|value| text(&value, "message").or_else(|| text(&value, "error")))
        });
        match message {
            Some(message) => bail!("{message}"),
            None => bail!("it answered {status}"),
        }
    }
    Ok(body)
}

async fn get_json(client: &Client, url: Url, key: Option<&str>) -> Result<Value> {
    let body = get_text(client, url, key).await?;
    serde_json::from_str(&body)
        .map_err(|_| anyhow!("it did not answer with search results; check the address"))
}

fn describe(err: reqwest::Error) -> anyhow::Error {
    if err.is_timeout() {
        anyhow!("it took too long to answer")
    } else if err.is_connect() {
        anyhow!("cannot reach it; check the address and that it is running")
    } else {
        anyhow!("{err}")
    }
}

/// The description of a Torznab `<error code=".." description=".."/>` answer.
fn torznab_error(body: &str) -> Option<String> {
    let start = body.find("<error")?;
    let rest = &body[start..];
    let at = rest.find("description=\"")? + "description=\"".len();
    let end = rest[at..].find('"')?;
    Some(rest[at..at + end].to_string())
}

/// Prowlarr writes camelCase and Jackett PascalCase, so fields are matched without case.
fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

fn text(value: &Value, name: &str) -> Option<String> {
    field(value, name)?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn number(value: &Value, name: &str) -> Option<u64> {
    match field(value, name)? {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_f64().filter(|n| *n >= 0.0).map(|n| n as u64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn count(value: &Value, name: &str) -> Option<u32> {
    number(value, name).map(|n| n.min(u32::MAX as u64) as u32)
}

fn is_torrent(item: &Value) -> bool {
    match field(item, "protocol") {
        Some(Value::String(protocol)) => protocol.eq_ignore_ascii_case("torrent"),
        Some(Value::Number(protocol)) => protocol.as_u64() == Some(2),
        _ => true,
    }
}

fn category(value: Option<&Value>) -> SearchCategory {
    let ids = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| match entry {
            Value::Number(id) => id.as_u64(),
            other => number(other, "id"),
        });
    ids.filter(|id| *id < 10000)
        .map(|id| SearchCategory::from_newznab(id as u32))
        .next()
        .unwrap_or(SearchCategory::Other)
}

fn date(text: &str) -> Option<i64> {
    let seconds = chrono::DateTime::parse_from_rfc3339(text)
        .map(|at| at.timestamp())
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|at| at.and_utc().timestamp())
        })
        .ok()?;
    (seconds > 0).then_some(seconds)
}

pub fn parse_prowlarr(value: &Value, via: &str) -> Vec<SearchResult> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter(|item| is_torrent(item))
        .filter_map(|item| {
            Some(SearchResult {
                title: text(item, "title")?,
                link: text(item, "magnetUrl").or_else(|| text(item, "downloadUrl"))?,
                size: number(item, "size").unwrap_or(0),
                seeds: count(item, "seeders"),
                peers: count(item, "leechers"),
                published: text(item, "publishDate").and_then(|at| date(&at)),
                source: text(item, "indexer").unwrap_or_else(|| via.to_string()),
                via: via.to_string(),
                info_url: text(item, "infoUrl").or_else(|| text(item, "commentUrl")),
                info_hash: text(item, "infoHash").map(|hash| hash.to_ascii_lowercase()),
                category: category(field(item, "categories")),
            })
        })
        .collect()
}

pub fn parse_jackett(value: &Value, via: &str) -> (Vec<SearchResult>, Vec<SearchFailure>) {
    let results = field(value, "results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(SearchResult {
                title: text(item, "title")?,
                link: text(item, "magnetUri").or_else(|| text(item, "link"))?,
                size: number(item, "size").unwrap_or(0),
                seeds: count(item, "seeders"),
                peers: count(item, "peers"),
                published: text(item, "publishDate").and_then(|at| date(&at)),
                source: text(item, "tracker").unwrap_or_else(|| via.to_string()),
                via: via.to_string(),
                info_url: text(item, "details"),
                info_hash: text(item, "infoHash").map(|hash| hash.to_ascii_lowercase()),
                category: category(field(item, "category")),
            })
        })
        .collect();
    let failures = field(value, "indexers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|indexer| number(indexer, "status") == Some(1))
        .map(|indexer| SearchFailure {
            source: text(indexer, "name").unwrap_or_else(|| via.to_string()),
            error: text(indexer, "error")
                .map(|error| first_line(&error))
                .unwrap_or_else(|| "the search failed".into()),
        })
        .collect();
    (results, failures)
}

/// The readable part of a Jackett error, without the .NET exception names in front.
fn first_line(text: &str) -> String {
    let mut line = text.lines().next().unwrap_or_default().trim();
    while let Some((head, rest)) = line.split_once(": ") {
        let class = !head.contains(' ') && head.ends_with("Exception");
        let tracker = head.starts_with("Exception (") && head.ends_with(')');
        if !(class || tracker) {
            break;
        }
        line = rest.trim();
    }
    line.chars().take(200).collect()
}

/// One row per torrent: the same info hash, or the same name and size, from several trackers.
pub fn merge(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut merged: Vec<SearchResult> = Vec::with_capacity(results.len());
    let mut seen: HashMap<String, usize> = HashMap::new();
    for result in results {
        let key = result
            .info_hash
            .clone()
            .unwrap_or_else(|| format!("{}|{}", result.title.to_lowercase(), result.size));
        match seen.get(&key) {
            Some(&ix) => {
                let kept = &mut merged[ix];
                let mut sources: Vec<String> =
                    kept.source.split(", ").map(str::to_string).collect();
                if !sources.contains(&result.source) {
                    sources.push(result.source.clone());
                }
                if result.seeds.unwrap_or(0) > kept.seeds.unwrap_or(0) {
                    let peers = kept.peers.max(result.peers);
                    *kept = result;
                    kept.peers = peers;
                } else {
                    kept.peers = kept.peers.max(result.peers);
                }
                kept.source = sources.join(", ");
            }
            None => {
                seen.insert(key, merged.len());
                merged.push(result);
            }
        }
    }
    merged.sort_by(|a, b| {
        b.seeds
            .unwrap_or(0)
            .cmp(&a.seeds.unwrap_or(0))
            .then(b.published.unwrap_or(0).cmp(&a.published.unwrap_or(0)))
    });
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const PROWLARR: &str = r#"[
      {"guid":"a","age":3,"size":1610612736,"indexerId":4,"indexer":"1337x","title":"Ubuntu 24.04 Desktop","publishDate":"2026-10-30T12:00:00Z",
       "downloadUrl":"http://tower:9696/4/download?apikey=k&link=x","magnetUrl":"http://tower:9696/4/download?apikey=k&link=m",
       "infoUrl":"https://example.org/t/1","infoHash":"DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C","seeders":120,"leechers":8,
       "protocol":"torrent","categories":[{"id":4000,"name":"PC"},{"id":100001,"name":"Other"}]},
      {"guid":"b","size":10,"indexer":"nzbgeek","title":"Usenet thing","downloadUrl":"http://x/nzb","protocol":"usenet"},
      {"guid":"c","size":5,"indexer":"tpb","title":"no link","protocol":"torrent"}
    ]"#;

    const JACKETT: &str = r#"{"Results":[
      {"FirstSeen":"0001-01-01T00:00:00","Tracker":"The Pirate Bay","TrackerId":"thepiratebay","Title":"Ubuntu 24.04 Desktop",
       "Link":"http://tower:9117/dl/thepiratebay/?jackett_apikey=k&path=p","Details":"https://example.net/1",
       "PublishDate":"2026-10-31T08:30:00+00:00","Category":[4000,100001],"Size":1610612736,"Seeders":40,"Peers":30,
       "InfoHash":"dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c","MagnetUri":null},
      {"Tracker":"EZTV","Title":"Some Show S01E01","Link":"magnet:?xt=urn:btih:aaaa","PublishDate":"0001-01-01T00:00:00",
       "Category":[5000],"Size":734003200,"Seeders":15,"Peers":2}
     ],
     "Indexers":[{"ID":"eztv","Name":"EZTV","Status":2,"Results":1,"Error":null},
                 {"ID":"limetorrents","Name":"LimeTorrents","Status":1,"Results":0,"Error":"Jackett.Common.IndexerException: Exception (limetorrents): timed out\n at stack"}]}"#;

    fn request(query: &str, category: SearchCategory) -> SearchRequest {
        SearchRequest {
            query: query.into(),
            category,
        }
    }

    #[test]
    fn builds_prowlarr_urls_under_a_base_path() {
        let url = prowlarr_search_url(
            "tower:9696/prowlarr/",
            &request(" ubuntu iso ", SearchCategory::Games),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "http://tower:9696/prowlarr/api/v1/search?query=ubuntu+iso&type=search&limit=100&categories=1000&categories=4050"
        );
    }

    #[test]
    fn builds_jackett_urls() {
        let url = jackett_search_url(
            "https://box.lan:9117",
            "KEY",
            &request("debian", SearchCategory::Tv),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://box.lan:9117/api/v2.0/indexers/all/results?apikey=KEY&query=debian&Category%5B%5D=5000"
        );
        assert!(jackett_search_url("ftp://box", "k", &request("x", SearchCategory::All)).is_err());
    }

    #[test]
    fn reads_prowlarr_results() {
        let value: Value = serde_json::from_str(PROWLARR).unwrap();
        let results = parse_prowlarr(&value, "prowlarr");
        assert_eq!(results.len(), 1, "usenet and link-less results are dropped");
        let first = &results[0];
        assert_eq!(first.title, "Ubuntu 24.04 Desktop");
        assert_eq!(first.link, "http://tower:9696/4/download?apikey=k&link=m");
        assert_eq!((first.seeds, first.peers), (Some(120), Some(8)));
        assert_eq!(first.source, "1337x");
        assert_eq!(first.category, SearchCategory::Software);
        assert_eq!(
            first.info_hash.as_deref(),
            Some("dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c")
        );
        assert_eq!(first.published, Some(1793361600));
    }

    #[test]
    fn reads_jackett_results_and_failures() {
        let value: Value = serde_json::from_str(JACKETT).unwrap();
        let (results, failures) = parse_jackett(&value, "jackett");
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].link,
            "http://tower:9117/dl/thepiratebay/?jackett_apikey=k&path=p"
        );
        assert_eq!(results[0].source, "The Pirate Bay");
        assert_eq!(results[1].category, SearchCategory::Tv);
        assert_eq!(
            results[1].published, None,
            "Jackett's empty date is year one"
        );
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].source, "LimeTorrents");
        assert_eq!(failures[0].error, "timed out");
        assert_eq!(
            first_line("Exception (x): Request to x failed (Error 503)"),
            "Request to x failed (Error 503)"
        );
        assert_eq!(first_line("site said: no"), "site said: no");
    }

    #[test]
    fn merges_the_same_torrent_from_two_trackers() {
        let prowlarr = parse_prowlarr(&serde_json::from_str(PROWLARR).unwrap(), "prowlarr");
        let (jackett, _) = parse_jackett(&serde_json::from_str(JACKETT).unwrap(), "jackett");
        let merged = merge(prowlarr.into_iter().chain(jackett).collect());
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].source, "1337x, The Pirate Bay");
        assert_eq!(merged[0].seeds, Some(120));
        assert_eq!(merged[0].peers, Some(30));
        assert_eq!(merged[1].title, "Some Show S01E01");
    }

    #[test]
    fn reads_torznab_errors() {
        let body = r#"<?xml version="1.0"?><error code="100" description="Invalid API Key" />"#;
        assert_eq!(torznab_error(body).as_deref(), Some("Invalid API Key"));
        assert_eq!(torznab_error("<caps></caps>"), None);
    }

    /// Answers each request with what `reply` returns for its request head.
    async fn serve(reply: fn(&str) -> String) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = stream.write_all(reply(&head).as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn respond(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn follows_redirects_to_magnets_and_files() {
        let base = serve(|head| {
            if head.starts_with("GET /magnet ") {
                respond("302 Found", "Location: /hop\r\n", "")
            } else if head.starts_with("GET /hop ") {
                respond(
                    "301 Moved",
                    "Location: magnet:?xt=urn:btih:abc&dn=a b\r\n",
                    "",
                )
            } else if head.starts_with("GET /file ") {
                respond("200 OK", "", "d4:infod4:name1:xee")
            } else {
                respond(
                    "200 OK",
                    "Content-Type: text/html\r\n",
                    "<html>login</html>",
                )
            }
        })
        .await;
        let client = client();
        match fetch_link(&client, &format!("{base}/magnet"))
            .await
            .unwrap()
        {
            Fetched::Magnet(link) => assert_eq!(link, "magnet:?xt=urn:btih:abc&dn=a b"),
            Fetched::Torrent(_) => panic!("expected a magnet"),
        }
        assert!(matches!(
            fetch_link(&client, &format!("{base}/file")).await.unwrap(),
            Fetched::Torrent(bytes) if bytes.starts_with(b"d4:info")
        ));
        let err = fetch_link(&client, &format!("{base}/page"))
            .await
            .err()
            .unwrap();
        assert!(err.to_string().contains("web page"));
    }

    #[tokio::test]
    async fn searches_a_prowlarr_server_and_reports_a_dead_one() {
        let base = serve(|head| {
            let authorized = head.to_ascii_lowercase().contains("x-api-key: secret");
            if head.starts_with("GET /api/v1/search?query=ubuntu&type=search") && authorized {
                respond("200 OK", "Content-Type: application/json\r\n", PROWLARR)
            } else if head.starts_with("GET /api/v1/indexer ") && authorized {
                respond("200 OK", "", r#"[{"enable":true,"protocol":"torrent"},{"enable":false,"protocol":"torrent"},{"enable":true,"protocol":"usenet"}]"#)
            } else {
                respond("401 Unauthorized", "", "")
            }
        })
        .await;
        let mut prowlarr = Indexer::new(IndexerKind::Prowlarr);
        prowlarr.url = base.clone();
        prowlarr.api_key = "secret".into();
        let mut dead = Indexer::new(IndexerKind::Jackett);
        dead.url = "http://127.0.0.1:9".into();
        dead.api_key = "k".into();
        let mut off = Indexer::new(IndexerKind::Jackett);
        off.enabled = false;

        let client = client();
        let reply = search(
            &client,
            &[prowlarr.clone(), dead, off],
            &request("ubuntu", SearchCategory::All),
        )
        .await;
        assert_eq!(reply.results.len(), 1);
        assert_eq!(reply.failures.len(), 1);
        assert_eq!(reply.failures[0].source, "jackett");

        assert_eq!(
            test(&client, &prowlarr).await.unwrap(),
            "connected: 1 torrent indexer"
        );
        prowlarr.api_key = "wrong".into();
        assert_eq!(
            test(&client, &prowlarr).await.unwrap_err().to_string(),
            "the API key was refused"
        );
    }
}
