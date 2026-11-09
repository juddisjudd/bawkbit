pub mod api;
mod engine;
mod geoip;
mod magnet;
mod rss;
mod search;
pub mod settings;
mod store;
pub mod trackers;

use std::path::PathBuf;

pub use api::*;
pub use engine::Core;
pub use settings::Settings;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `BAWKBIT_HOME` moves settings and the session elsewhere, so a second copy or a test run leaves the real ones alone.
pub fn data_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("BAWKBIT_HOME") {
        return PathBuf::from(home);
    }
    dirs::config_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_default()
        .join("bawkbit")
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}
