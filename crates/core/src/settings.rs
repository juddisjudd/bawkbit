use std::path::{Path, PathBuf};

use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};

pub const DEFAULT_WEB_PORT: u16 = 8228;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub download_dir: String,
    pub start_paused: bool,
    pub listen_port: u16,
    pub upnp: bool,
    pub dht: bool,
    /// Most peers one torrent connects to; 0 leaves it to the engine.
    pub peer_limit: u32,
    /// Bytes per second; 0 means no limit.
    pub down_limit: u64,
    pub up_limit: u64,
    pub alt_down_limit: u64,
    pub alt_up_limit: u64,
    pub alt_speed: bool,
    pub schedule: Schedule,
    /// Unfinished torrents allowed to download at once; 0 means no queue.
    pub max_active_downloads: u32,
    /// Stop seeding at this upload to download ratio; 0 seeds forever.
    pub ratio_limit: f32,
    /// Stop seeding after this many minutes; 0 seeds forever.
    pub seed_minutes: u32,
    /// A folder whose .torrent files are added on their own; empty turns it off.
    pub watch_dir: String,
    pub web: Web,
    /// Prowlarr and Jackett servers that search answers come from.
    pub indexers: Vec<Indexer>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: default_download_dir().to_string_lossy().into_owned(),
            start_paused: false,
            listen_port: random_port(),
            upnp: true,
            dht: true,
            peer_limit: 0,
            down_limit: 0,
            up_limit: 0,
            alt_down_limit: 1024 * 1024,
            alt_up_limit: 256 * 1024,
            alt_speed: false,
            schedule: Schedule::default(),
            max_active_downloads: 5,
            ratio_limit: 0.0,
            seed_minutes: 0,
            watch_dir: String::new(),
            web: Web::default(),
            indexers: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    /// Minutes after midnight, local time.
    pub from: u16,
    pub to: u16,
    /// One bit per weekday, Monday first.
    pub days: u8,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            enabled: false,
            from: 8 * 60,
            to: 20 * 60,
            days: 0b0111_1111,
        }
    }
}

impl Schedule {
    pub fn covers(&self, weekday: u32, minute: u16) -> bool {
        if !self.enabled || self.days & (1 << weekday) == 0 {
            return false;
        }
        if self.from <= self.to {
            (self.from..self.to).contains(&minute)
        } else {
            minute >= self.from || minute < self.to
        }
    }

    pub fn covers_now(&self) -> bool {
        let now = chrono::Local::now();
        let minute = (now.hour() * 60 + now.minute()) as u16;
        self.covers(now.weekday().num_days_from_monday(), minute)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Web {
    pub enabled: bool,
    pub address: String,
    pub port: u16,
    pub username: String,
    /// Never sent to clients; see `Settings::public`.
    pub password_hash: String,
    /// A new password from a client, hashed before it is stored.
    #[serde(skip_serializing)]
    pub password: Option<String>,
    /// Lets browsers on this computer in without a password.
    pub local_bypass: bool,
}

impl Default for Web {
    fn default() -> Self {
        Self {
            enabled: false,
            address: "0.0.0.0".into(),
            port: DEFAULT_WEB_PORT,
            username: "admin".into(),
            password_hash: String::new(),
            password: None,
            local_bypass: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexerKind {
    #[default]
    Prowlarr,
    Jackett,
}

impl IndexerKind {
    pub fn label(self) -> &'static str {
        match self {
            IndexerKind::Prowlarr => "prowlarr",
            IndexerKind::Jackett => "jackett",
        }
    }

    pub fn default_url(self) -> &'static str {
        match self {
            IndexerKind::Prowlarr => "http://localhost:9696",
            IndexerKind::Jackett => "http://localhost:9117",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Indexer {
    pub id: String,
    pub kind: IndexerKind,
    pub name: String,
    /// The address its own web page opens at, with any base path, such as http://tower:9696.
    pub url: String,
    /// Never sent to clients; an empty key from a client keeps the stored one.
    pub api_key: String,
    /// Tells a client a key is stored without sending it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub has_key: bool,
    pub enabled: bool,
}

impl Indexer {
    pub fn new(kind: IndexerKind) -> Self {
        Self {
            id: new_id(),
            kind,
            name: kind.label().into(),
            url: kind.default_url().into(),
            api_key: String::new(),
            has_key: false,
            enabled: true,
        }
    }
}

fn new_id() -> String {
    let mut bytes = [0u8; 6];
    let _ = getrandom::fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Settings {
    pub fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::store::write_atomic(path, &json)
    }

    /// A copy safe to hand to a client.
    pub fn public(&self) -> Self {
        let mut copy = self.clone();
        copy.web.password_hash.clear();
        copy.web.password = None;
        for indexer in &mut copy.indexers {
            indexer.has_key = !indexer.api_key.is_empty();
            indexer.api_key.clear();
        }
        copy
    }

    /// Takes a client's settings, keeping what clients never see.
    pub fn merge_from_client(&self, mut incoming: Settings) -> Settings {
        incoming.web.password_hash = self.web.password_hash.clone();
        for indexer in &mut incoming.indexers {
            if indexer.id.is_empty() {
                indexer.id = new_id();
            }
            if indexer.api_key.is_empty()
                && let Some(stored) = self.indexers.iter().find(|stored| stored.id == indexer.id)
            {
                indexer.api_key = stored.api_key.clone();
            }
            indexer.has_key = false;
        }
        incoming
    }

    pub fn indexer(&self, id: &str) -> Option<&Indexer> {
        self.indexers.iter().find(|indexer| indexer.id == id)
    }

    pub fn effective_limits(&self) -> (u64, u64) {
        if self.alt_speed {
            (self.alt_down_limit, self.alt_up_limit)
        } else {
            (self.down_limit, self.up_limit)
        }
    }

    pub fn needs_new_session(&self, other: &Settings) -> bool {
        self.listen_port != other.listen_port
            || self.upnp != other.upnp
            || self.dht != other.dht
            || self.peer_limit != other.peer_limit
    }
}

pub fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_default()
        .join("bawkbit")
}

/// A port from the dynamic range, picked once, so bawkbit does not sit on the 6881 that some networks throttle.
fn random_port() -> u16 {
    let mut bytes = [0u8; 2];
    let _ = getrandom::fill(&mut bytes);
    49152 + u16::from_le_bytes(bytes) % 16000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_covers_a_daytime_window() {
        let schedule = Schedule {
            enabled: true,
            from: 8 * 60,
            to: 20 * 60,
            days: 0b0001_1111,
        };
        assert!(schedule.covers(0, 9 * 60));
        assert!(!schedule.covers(0, 21 * 60));
        assert!(!schedule.covers(5, 9 * 60));
    }

    #[test]
    fn schedule_wraps_past_midnight() {
        let schedule = Schedule {
            enabled: true,
            from: 22 * 60,
            to: 6 * 60,
            days: 0b0111_1111,
        };
        assert!(schedule.covers(3, 23 * 60));
        assert!(schedule.covers(3, 60));
        assert!(!schedule.covers(3, 12 * 60));
    }

    #[test]
    fn clients_never_see_or_clear_the_password() {
        let mut settings = Settings::default();
        settings.web.password_hash = "secret".into();
        let public = settings.public();
        assert!(public.web.password_hash.is_empty());
        assert_eq!(
            settings.merge_from_client(public).web.password_hash,
            "secret"
        );
    }

    #[test]
    fn clients_never_see_indexer_keys_and_keep_them_by_sending_none() {
        let mut settings = Settings::default();
        let mut indexer = Indexer::new(IndexerKind::Jackett);
        indexer.api_key = "key".into();
        settings.indexers.push(indexer);
        let mut public = settings.public();
        assert!(public.indexers[0].api_key.is_empty());
        assert!(public.indexers[0].has_key);
        let mut added = Indexer::new(IndexerKind::Prowlarr);
        added.id.clear();
        public.indexers.push(added);
        let merged = settings.merge_from_client(public);
        assert_eq!(merged.indexers[0].api_key, "key");
        assert!(!merged.indexers[1].id.is_empty());
        assert!(merged.indexers.iter().all(|indexer| !indexer.has_key));
    }

    #[test]
    fn old_files_fill_in_new_fields() {
        let settings: Settings = serde_json::from_str(r#"{"listen_port": 50000}"#).unwrap();
        assert_eq!(settings.listen_port, 50000);
        assert_eq!(settings.web.port, DEFAULT_WEB_PORT);
    }
}
