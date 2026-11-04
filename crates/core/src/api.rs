use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// A torrent's info hash in lowercase hex: stable across restarts and the same for every client.
pub type Id = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Metadata,
    Checking,
    Downloading,
    Stalled,
    Seeding,
    Done,
    Paused,
    Queued,
    Error,
}

impl State {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            State::Metadata | State::Checking | State::Downloading | State::Seeding
        )
    }

    pub fn is_paused(self) -> bool {
        matches!(self, State::Paused | State::Done)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Torrent {
    pub id: Id,
    pub name: String,
    pub state: State,
    pub progress: f32,
    pub size: u64,
    pub done: u64,
    pub uploaded: u64,
    pub down: u64,
    pub up: u64,
    pub eta: Option<u64>,
    pub ratio: f32,
    pub peers: u32,
    pub seeds: u32,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub added: i64,
    pub completed: Option<i64>,
    pub save_path: String,
    pub error: Option<String>,
    #[serde(default)]
    pub forced: bool,
    #[serde(default)]
    pub no_upload: bool,
    /// Bytes per second for this torrent alone; 0 means no limit.
    #[serde(default)]
    pub down_limit: u64,
    #[serde(default)]
    pub up_limit: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    pub down: u64,
    pub up: u64,
    pub down_limit: u64,
    pub up_limit: u64,
    pub alt_speed: bool,
    pub dht_nodes: usize,
    pub downloaded: u64,
    pub uploaded: u64,
    pub free_space: Option<u64>,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub save_path: String,
}

/// Everything a list view needs, sent once a second.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub rev: u64,
    pub torrents: Vec<Torrent>,
    pub totals: Totals,
    pub categories: Vec<Category>,
    pub tags: Vec<String>,
    /// Download and upload speed for each of the last few minutes' seconds, oldest first.
    pub history: Vec<[u64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct File {
    pub index: usize,
    pub path: String,
    pub size: u64,
    pub done: u64,
    pub included: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub addr: String,
    pub client: Option<String>,
    pub transport: Option<String>,
    pub down: u64,
    pub up: u64,
    pub downloaded: u64,
    pub uploaded: u64,
}

/// One torrent in full, fetched while its details are open.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Details {
    pub id: Id,
    pub name: String,
    pub save_path: String,
    pub magnet: String,
    pub size: u64,
    pub piece_size: u32,
    pub pieces: u32,
    /// How much of each slice of the torrent is here, 0 to 255, for drawing a piece map.
    pub piece_map: Vec<u8>,
    pub files: Vec<File>,
    pub peers: Vec<Peer>,
    pub trackers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Source {
    /// A magnet link, an info hash, or an http(s) link to a .torrent file.
    Link(String),
    /// The bytes of a .torrent file, base64 in JSON.
    File(#[serde(with = "base64_bytes")] Vec<u8>),
    /// A torrent already resolved by `preview`, by its id.
    Resolved(Id),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreviewFile {
    pub index: usize,
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    pub id: Id,
    pub name: String,
    pub size: u64,
    pub files: Vec<PreviewFile>,
    pub save_path: String,
    pub existing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AddRequest {
    pub source: Source,
    #[serde(default)]
    pub save_path: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub paused: Option<bool>,
    /// Indexes of the files to download; all of them when absent.
    #[serde(default)]
    pub files: Option<Vec<usize>>,
}

impl AddRequest {
    pub fn link(link: impl Into<String>) -> Self {
        Self {
            source: Source::Link(link.into()),
            save_path: None,
            category: None,
            tags: Vec::new(),
            paused: None,
            files: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Pause {
        ids: Vec<Id>,
    },
    Resume {
        ids: Vec<Id>,
    },
    Remove {
        ids: Vec<Id>,
        #[serde(default)]
        delete_files: bool,
    },
    SetFiles {
        id: Id,
        files: Vec<usize>,
    },
    SetCategory {
        ids: Vec<Id>,
        category: Option<String>,
    },
    AddTags {
        ids: Vec<Id>,
        tags: Vec<String>,
    },
    RemoveTags {
        ids: Vec<Id>,
        tags: Vec<String>,
    },
    SaveCategory {
        name: String,
        #[serde(default)]
        save_path: Option<String>,
    },
    RemoveCategory {
        name: String,
    },
    RemoveTag {
        tag: String,
    },
    SetAltSpeed {
        on: bool,
    },
    PauseAll,
    ResumeAll,
    /// Starts torrents even when the download queue is full.
    ForceStart {
        ids: Vec<Id>,
    },
    /// Checks every piece on disk again.
    Recheck {
        ids: Vec<Id>,
    },
    /// Replaces a torrent's trackers; it keeps its pieces and carries on.
    SetTrackers {
        id: Id,
        trackers: Vec<String>,
    },
    SetNoUpload {
        ids: Vec<Id>,
        on: bool,
    },
    /// Bytes per second; 0 means no limit.
    SetLimits {
        ids: Vec<Id>,
        down: u64,
        up: u64,
    },
}

/// The answer to any request that changes something.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Done,
    Added { id: Id },
    Preview(Preview),
    Details(Box<Details>),
    Settings(Box<Settings>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchCategory {
    #[default]
    All,
    Movies,
    Tv,
    Music,
    Books,
    Software,
    Games,
    Other,
}

impl SearchCategory {
    pub const ALL: [SearchCategory; 8] = [
        SearchCategory::All,
        SearchCategory::Movies,
        SearchCategory::Tv,
        SearchCategory::Music,
        SearchCategory::Books,
        SearchCategory::Software,
        SearchCategory::Games,
        SearchCategory::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SearchCategory::All => "all",
            SearchCategory::Movies => "movies",
            SearchCategory::Tv => "tv",
            SearchCategory::Music => "music",
            SearchCategory::Books => "books",
            SearchCategory::Software => "software",
            SearchCategory::Games => "games",
            SearchCategory::Other => "other",
        }
    }

    /// The Newznab category numbers indexers file this under.
    pub fn newznab(self) -> &'static [u32] {
        match self {
            SearchCategory::All => &[],
            SearchCategory::Movies => &[2000],
            SearchCategory::Tv => &[5000],
            SearchCategory::Music => &[3000],
            SearchCategory::Books => &[7000],
            SearchCategory::Software => &[4000],
            SearchCategory::Games => &[1000, 4050],
            SearchCategory::Other => &[8000],
        }
    }

    pub fn from_newznab(id: u32) -> Self {
        match id {
            4050..=4059 | 1000..=1999 => SearchCategory::Games,
            2000..=2999 => SearchCategory::Movies,
            3000..=3999 => SearchCategory::Music,
            4000..=4999 => SearchCategory::Software,
            5000..=5999 => SearchCategory::Tv,
            7000..=7999 => SearchCategory::Books,
            _ => SearchCategory::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub category: SearchCategory,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub size: u64,
    pub seeds: Option<u32>,
    pub peers: Option<u32>,
    /// Unix seconds.
    pub published: Option<i64>,
    /// The trackers that list it, as the indexer names them.
    pub source: String,
    /// The Prowlarr or Jackett server it came through.
    pub via: String,
    /// A magnet link or a link to the .torrent file; add it as `Source::Link`.
    pub link: String,
    pub info_url: Option<String>,
    pub info_hash: Option<String>,
    pub category: SearchCategory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchFailure {
    pub source: String,
    pub error: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchReply {
    /// Most seeds first; a torrent listed by several trackers shows once.
    pub results: Vec<SearchResult>,
    pub failures: Vec<SearchFailure>,
}

mod base64_bytes {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD
            .decode(text.trim())
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_use_a_flat_tag() {
        let json = r#"{"action":"remove","ids":["ab"],"delete_files":true}"#;
        let action: Action = serde_json::from_str(json).unwrap();
        assert_eq!(
            action,
            Action::Remove {
                ids: vec!["ab".into()],
                delete_files: true
            }
        );
        let json = serde_json::to_string(&Action::PauseAll).unwrap();
        assert_eq!(json, r#"{"action":"pause_all"}"#);
    }

    #[test]
    fn file_sources_travel_as_base64() {
        let source = Source::File(b"d4:infoe".to_vec());
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, r#"{"kind":"file","value":"ZDQ6aW5mb2U="}"#);
        assert_eq!(serde_json::from_str::<Source>(&json).unwrap(), source);
    }
}
