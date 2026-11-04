use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::api::Id;

/// What bawkbit knows about its torrents beyond what the engine keeps.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub torrents: HashMap<Id, Entry>,
    /// Category name to its save path; an empty path means a folder of that name in the download folder.
    pub categories: BTreeMap<String, String>,
    pub tags: BTreeSet<String>,
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
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
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

/// Writes next to the target and renames, so a crash never leaves half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
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
        let loaded = Store::load(&path);
        assert_eq!(loaded.torrents["abc"].tags, vec!["linux".to_string()]);
        assert_eq!(loaded.all_tags(), vec!["linux".to_string()]);
        assert!(loaded.categories.contains_key("iso"));
    }
}
