use std::collections::{HashMap, HashSet};

use axum::response::sse::Event;
use bawkbit_core::{Category, Id, Snapshot, Torrent, Totals};
use serde::Serialize;

/// What one page was last sent, so each event after the first carries only what changed.
#[derive(Default)]
pub struct Sent {
    /// 0 until the first snapshot goes out; the engine's revisions start at 1.
    rev: u64,
    torrents: HashMap<Id, Torrent>,
    categories: Vec<Category>,
    tags: Vec<String>,
}

/// The part of a snapshot that differs from the one before it.
#[derive(Debug, Serialize)]
pub struct Changes<'a> {
    pub rev: u64,
    /// Rows that are new or changed.
    pub torrents: Vec<&'a Torrent>,
    /// Ids of rows that went away.
    pub removed: Vec<Id>,
    pub totals: &'a Totals,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<&'a [Category]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<&'a [String]>,
    /// Speed samples since the last event, oldest first.
    pub history: &'a [[u64; 2]],
    pub rss_rev: u64,
}

impl Sent {
    /// A full `snapshot` event the first time, and a `changes` event after that.
    pub fn event(&mut self, snapshot: &Snapshot) -> Option<Event> {
        if self.rev == 0 {
            self.rev = snapshot.rev;
            self.torrents = snapshot
                .torrents
                .iter()
                .map(|torrent| (torrent.id.clone(), torrent.clone()))
                .collect();
            self.categories = snapshot.categories.clone();
            self.tags = snapshot.tags.clone();
            return Event::default().event("snapshot").json_data(snapshot).ok();
        }
        Event::default()
            .event("changes")
            .json_data(self.changes(snapshot))
            .ok()
    }

    pub fn changes<'a>(&mut self, snapshot: &'a Snapshot) -> Changes<'a> {
        let torrents: Vec<&Torrent> = snapshot
            .torrents
            .iter()
            .filter(|torrent| self.torrents.get(&torrent.id) != Some(*torrent))
            .collect();
        for torrent in &torrents {
            self.torrents.insert(torrent.id.clone(), (*torrent).clone());
        }
        let present: HashSet<&str> = snapshot.torrents.iter().map(|t| t.id.as_str()).collect();
        let removed: Vec<Id> = self
            .torrents
            .keys()
            .filter(|id| !present.contains(id.as_str()))
            .cloned()
            .collect();
        for id in &removed {
            self.torrents.remove(id);
        }
        let categories = (snapshot.categories != self.categories).then(|| {
            self.categories = snapshot.categories.clone();
            &snapshot.categories[..]
        });
        let tags = (snapshot.tags != self.tags).then(|| {
            self.tags = snapshot.tags.clone();
            &snapshot.tags[..]
        });
        let samples = (snapshot.rev.saturating_sub(self.rev) as usize).min(snapshot.history.len());
        self.rev = snapshot.rev;
        Changes {
            rev: snapshot.rev,
            torrents,
            removed,
            totals: &snapshot.totals,
            categories,
            tags,
            history: &snapshot.history[snapshot.history.len() - samples..],
            rss_rev: snapshot.rss_rev,
        }
    }
}

#[cfg(test)]
mod tests {
    use bawkbit_core::State;

    use super::*;

    fn row(id: &str, down: u64) -> Torrent {
        Torrent {
            id: id.into(),
            name: id.into(),
            state: State::Downloading,
            progress: 0.0,
            size: 10,
            done: 0,
            uploaded: 0,
            down,
            up: 0,
            eta: None,
            ratio: 0.0,
            peers: 0,
            seeds: 0,
            category: None,
            tags: Vec::new(),
            added: 0,
            completed: None,
            save_path: String::new(),
            error: None,
            forced: false,
            no_upload: false,
            down_limit: 0,
            up_limit: 0,
        }
    }

    #[test]
    fn after_the_first_snapshot_only_changes_go_out() {
        let mut snapshot = Snapshot {
            rev: 7,
            torrents: vec![row("a", 0), row("b", 0)],
            tags: vec!["linux".into()],
            history: vec![[1, 1], [2, 2]],
            ..Snapshot::default()
        };
        let mut sent = Sent::default();
        assert!(sent.event(&snapshot).is_some());

        snapshot.rev = 9;
        snapshot.torrents = vec![row("a", 5), row("c", 0)];
        snapshot.history = vec![[2, 2], [3, 3], [4, 4]];
        let changes = sent.changes(&snapshot);
        let ids: Vec<&str> = changes.torrents.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
        assert_eq!(changes.removed, ["b"]);
        assert_eq!(changes.history, [[3, 3], [4, 4]]);
        assert!(changes.tags.is_none());
        assert!(changes.categories.is_none());

        snapshot.rev = 10;
        snapshot.tags.clear();
        let changes = sent.changes(&snapshot);
        assert!(changes.torrents.is_empty());
        assert!(changes.removed.is_empty());
        assert_eq!(changes.tags, Some(&[][..]));
        let json = serde_json::to_value(&changes).unwrap();
        assert!(json.get("categories").is_none());
    }
}
