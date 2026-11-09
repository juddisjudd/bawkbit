use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::api::TrackerLists;

pub const NGOSANG_BEST: &str =
    "https://raw.githubusercontent.com/ngosang/trackerslist/master/trackers_best.txt";
pub const XIU2_BEST: &str =
    "https://raw.githubusercontent.com/XIU2/TrackersListCollection/master/best.txt";

const REFRESH_SECONDS: i64 = 24 * 60 * 60;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Cache {
    /// Each list's address and the trackers it gave last time it answered.
    lists: BTreeMap<String, Vec<String>>,
    fetched: Option<i64>,
    errors: Vec<String>,
}

/// Public tracker lists, fetched once a day and kept on disk.
pub struct Lists {
    path: PathBuf,
    cache: Mutex<Cache>,
    fetching: tokio::sync::Mutex<()>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Lists {
    pub fn load(path: PathBuf) -> Self {
        let cache = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            cache: Mutex::new(cache),
            fetching: tokio::sync::Mutex::new(()),
        }
    }

    /// The trackers the lists gave last time, without asking them again.
    pub fn cached(&self, lists: &[String]) -> Vec<String> {
        let cache = lock(&self.cache);
        let given = lists
            .iter()
            .filter_map(|list| cache.lists.get(list.trim()))
            .flatten();
        merge(&[], given)
    }

    pub fn info(&self, lists: &[String]) -> TrackerLists {
        let trackers = self.cached(lists).len();
        let cache = lock(&self.cache);
        TrackerLists {
            trackers,
            fetched: cache.fetched,
            errors: cache.errors.clone(),
        }
    }

    fn is_fresh(&self, lists: &[String]) -> bool {
        let cache = lock(&self.cache);
        let all_there = lists
            .iter()
            .all(|list| cache.lists.contains_key(list.trim()));
        let recent = cache
            .fetched
            .is_some_and(|at| crate::now() - at < REFRESH_SECONDS);
        all_there && recent
    }

    /// Asks each list again when a day has passed or the lists changed, or always with `force`.
    pub async fn refresh(&self, http: &reqwest::Client, lists: &[String], force: bool) {
        let _guard = self.fetching.lock().await;
        if !force && self.is_fresh(lists) {
            return;
        }
        let mut answers = Vec::new();
        for list in lists
            .iter()
            .map(|list| list.trim())
            .filter(|l| !l.is_empty())
        {
            answers.push((list.to_string(), fetch(http, list).await));
        }
        let mut cache = lock(&self.cache);
        cache.errors.clear();
        cache
            .lists
            .retain(|url, _| lists.iter().any(|list| list.trim() == url));
        for (list, answer) in answers {
            match answer {
                Ok(trackers) => {
                    cache.lists.insert(list, trackers);
                }
                Err(err) => {
                    tracing::warn!("cannot read the tracker list {list}: {err:#}");
                    cache.errors.push(format!("{list}: {err:#}"));
                }
            }
        }
        cache.fetched = Some(crate::now());
        let json = serde_json::to_vec_pretty(&*cache).unwrap_or_default();
        drop(cache);
        if let Err(err) = crate::store::write_atomic(&self.path, &json) {
            tracing::warn!("cannot save {:?}: {err}", self.path);
        }
    }
}

async fn fetch(http: &reqwest::Client, list: &str) -> Result<Vec<String>> {
    let response = http.get(list).send().await?;
    let status = response.status();
    if !status.is_success() {
        bail!("it answered {status}");
    }
    let trackers = parse(&response.text().await?);
    if trackers.is_empty() {
        bail!("it lists no trackers bawkbit can use");
    }
    Ok(trackers)
}

/// One tracker address a line; blank lines, comments and kinds the engine cannot use are skipped.
pub fn parse(text: &str) -> Vec<String> {
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .map(String::from)
        .collect::<Vec<_>>();
    merge(&[], &lines)
}

/// The address the engine uses for a tracker, or none when it is not an http, https or udp address.
pub fn normalize(tracker: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(tracker.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https" | "udp") && url.has_host())?;
    // udp is not a scheme the URL standard knows, so its host keeps the case it was typed in
    if let Some(host) = url.host_str().map(str::to_ascii_lowercase) {
        url.set_host(Some(&host)).ok()?;
    }
    Some(url.to_string())
}

/// `own` followed by each of `more` it does not have yet, all in the engine's form.
pub fn merge<'a>(own: &'a [String], more: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();
    for tracker in own.iter().chain(more) {
        if let Some(url) = normalize(tracker)
            && !merged.contains(&url)
        {
            merged.push(url);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_are_read_without_repeats_or_unusable_kinds() {
        let text = "udp://tracker.opentrackr.org:1337/announce\n\n\
            http://1337.abcvg.info:80/announce\n\n\
            wss://tracker.example.com:443/announce\n\
            # a comment\n\
            UDP://Tracker.OpenTrackr.org:1337/announce\n\
            not a tracker\n";
        assert_eq!(
            parse(text),
            [
                "udp://tracker.opentrackr.org:1337/announce",
                "http://1337.abcvg.info/announce"
            ]
        );
    }

    #[test]
    fn merging_keeps_the_torrent_trackers_first_and_adds_only_new_ones() {
        let own = vec!["http://1337.abcvg.info:80/announce".to_string()];
        let more = vec![
            "http://1337.abcvg.info/announce".to_string(),
            "udp://open.stealth.si:80/announce".to_string(),
        ];
        assert_eq!(
            merge(&own, &more),
            [
                "http://1337.abcvg.info/announce",
                "udp://open.stealth.si:80/announce"
            ]
        );
    }

    #[test]
    fn cached_lists_survive_a_restart_and_only_count_the_lists_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tracker-lists.json");
        let cache = Cache {
            lists: BTreeMap::from([
                (
                    "a".to_string(),
                    vec!["udp://one.example:1/announce".to_string()],
                ),
                (
                    "b".to_string(),
                    vec![
                        "udp://one.example:1/announce".to_string(),
                        "udp://two.example:2/announce".to_string(),
                    ],
                ),
            ]),
            fetched: Some(crate::now()),
            errors: Vec::new(),
        };
        std::fs::write(&path, serde_json::to_vec(&cache).unwrap()).unwrap();
        let lists = Lists::load(path);
        assert_eq!(lists.cached(&["a".into()]).len(), 1);
        assert_eq!(lists.cached(&["a".into(), "b".into()]).len(), 2);
        assert!(lists.is_fresh(&["a".into(), "b".into()]));
        assert!(!lists.is_fresh(&["c".into()]));
        assert_eq!(lists.info(&["b".into()]).trackers, 2);
    }
}
