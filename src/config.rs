use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme::{DEFAULT_THEME, Mode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    Name,
    Size,
    Progress,
    Down,
    Up,
    Eta,
    Ratio,
    Peers,
    #[default]
    Added,
}

/// What only the desktop app remembers: the engine's own settings live with the engine.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: String,
    pub mode: Mode,
    pub sort: Column,
    pub descending: bool,
    /// Adds magnet links straight away instead of showing their files first.
    pub quick_add: bool,
    pub check_updates: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.into(),
            mode: Mode::Dark,
            sort: Column::Added,
            descending: true,
            quick_add: false,
            check_updates: true,
        }
    }
}

fn path() -> PathBuf {
    bawkbit_core::data_dir().join("desktop.json")
}

impl Config {
    pub fn load() -> Self {
        std::fs::read(path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Ok(json) = serde_json::to_vec_pretty(self) else {
            return;
        };
        let path = path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(err) = std::fs::write(&path, json) {
            log::error!("cannot save {path:?}: {err}");
        }
    }
}
