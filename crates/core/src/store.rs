use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{ErrorKind, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::api::{AddRequest, Id};

/// What bawkbit knows about its torrents beyond what the engine keeps.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub torrents: HashMap<Id, Entry>,
    /// Category name to its save path; an empty path means a folder of that name in the download folder.
    pub categories: BTreeMap<String, String>,
    /// Category name to the download and upload limits its torrents share, in bytes per second.
    pub category_limits: BTreeMap<String, (u64, u64)>,
    pub tags: BTreeSet<String>,
    /// Magnet links still fetching their file list, which the engine itself does not keep across a restart.
    pub waiting: BTreeMap<Id, Waiting>,
    /// The folder last picked in an add dialog, offered first in the next one.
    pub last_save_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Waiting {
    pub added: i64,
    pub request: AddRequest,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub added: i64,
    pub completed: Option<i64>,
    /// Bytes sent over every session, since the engine counts from zero after each pause.
    pub uploaded: u64,
    /// Paused by the download queue rather than by a person.
    pub queued: bool,
    /// Paused because it reached the seeding goal.
    pub done: bool,
    /// Resumed by hand after reaching the seeding goal, so the goal no longer applies.
    pub keep_seeding: bool,
    /// Runs whatever the download queue says.
    pub forced: bool,
    /// Never tells peers it has pieces, so nothing is uploaded.
    pub no_upload: bool,
    /// Bytes per second for this torrent alone; 0 means no limit.
    pub down_limit: u64,
    pub up_limit: u64,
}

impl Store {
    pub fn load(path: &Path) -> Self {
        read_json(path).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(path, &json)
    }

    pub fn entry(&mut self, id: &str) -> &mut Entry {
        self.torrents
            .entry(id.to_string())
            .or_insert_with(|| Entry {
                added: crate::now(),
                ..Entry::default()
            })
    }

    pub fn all_tags(&self) -> Vec<String> {
        let mut tags = self.tags.clone();
        for entry in self.torrents.values() {
            tags.extend(entry.tags.iter().cloned());
        }
        tags.into_iter().collect()
    }
}

/// Reads a JSON file; one that exists but cannot be read is moved aside, so the next save cannot wipe it.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let problem = match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => return Some(value),
            Err(err) => err.to_string(),
        },
        Err(err) if err.kind() == ErrorKind::NotFound => return None,
        Err(err) => err.to_string(),
    };
    let mut aside = path.as_os_str().to_owned();
    aside.push(format!(".bad-{}", crate::now()));
    match std::fs::rename(path, &aside) {
        Ok(()) => tracing::warn!("{path:?} is damaged ({problem}); kept it as {aside:?}"),
        Err(err) => tracing::warn!("{path:?} is damaged ({problem}) and cannot be kept: {err}"),
    }
    None
}

static NEXT_TMP: AtomicU64 = AtomicU64::new(0);

/// Writes next to the target, flushes and renames, so a crash or power cut never leaves half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Two saves of one file at once must not share a temporary file.
    let tmp = path.with_extension(format!("tmp{}", NEXT_TMP.fetch_add(1, Ordering::Relaxed)));
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("torrents.json");
        let mut store = Store::default();
        store.entry("abc").tags.push("linux".into());
        store.categories.insert("iso".into(), String::new());
        store.save(&path).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let loaded = Store::load(&path);
        assert_eq!(loaded.torrents["abc"].tags, vec!["linux".to_string()]);
        assert_eq!(loaded.all_tags(), vec!["linux".to_string()]);
        assert!(loaded.categories.contains_key("iso"));
    }

    #[test]
    fn a_damaged_file_is_kept_aside_instead_of_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("torrents.json");
        assert!(Store::load(&path).torrents.is_empty());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

        let damaged = b"{\"torrents\": {\"abc\": ";
        std::fs::write(&path, damaged).unwrap();
        assert!(Store::load(&path).torrents.is_empty());
        assert!(!path.exists());
        let kept: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].starts_with("torrents.json.bad-"), "{kept:?}");
        assert_eq!(std::fs::read(dir.path().join(&kept[0])).unwrap(), damaged);
    }
}
