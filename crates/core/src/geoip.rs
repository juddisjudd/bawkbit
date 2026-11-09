use std::io::Read as _;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use chrono::Months;
use maxminddb::{PathElement, Reader};

/// DB-IP puts out a new free database early each month.
const STALE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Peer countries from DB-IP's free country database, kept a month at a time.
pub struct Countries {
    path: PathBuf,
    reader: RwLock<Option<Arc<Reader<Vec<u8>>>>>,
    updating: AtomicBool,
}

impl Countries {
    pub fn load(path: PathBuf) -> Self {
        let reader = Reader::open_readfile(&path).ok().map(Arc::new);
        Self {
            path,
            reader: RwLock::new(reader),
            updating: AtomicBool::new(false),
        }
    }

    fn reader(&self) -> Option<Arc<Reader<Vec<u8>>>> {
        self.reader
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// The two-letter country code of a peer address such as `1.2.3.4:6881`.
    pub fn of(&self, addr: &str) -> Option<String> {
        let ip = match addr.parse::<SocketAddr>() {
            Ok(addr) => addr.ip(),
            Err(_) => addr.parse::<IpAddr>().ok()?,
        };
        let reader = self.reader()?;
        let path = [PathElement::Key("country"), PathElement::Key("iso_code")];
        let code: Option<&str> = reader
            .lookup(ip.to_canonical())
            .ok()?
            .decode_path(&path)
            .ok()?;
        code.map(str::to_ascii_uppercase)
    }

    fn is_stale(&self) -> bool {
        std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .map(|at| at.elapsed().unwrap_or_default() > STALE)
            .unwrap_or(true)
    }

    /// Downloads the database when it is missing or a month old; a failed download keeps the old one.
    pub async fn update(&self, http: &reqwest::Client) {
        if self.reader().is_some() && !self.is_stale() {
            return;
        }
        if self.updating.swap(true, Ordering::AcqRel) {
            return;
        }
        match self.download(http).await {
            Ok(reader) => {
                *self.reader.write().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(reader));
                tracing::info!("updated the peer country database");
            }
            Err(err) => tracing::warn!("cannot update the peer country database: {err:#}"),
        }
        self.updating.store(false, Ordering::Release);
    }

    async fn download(&self, http: &reqwest::Client) -> Result<Reader<Vec<u8>>> {
        let now = chrono::Utc::now();
        let months = [Some(now), now.checked_sub_months(Months::new(1))];
        let mut failure = anyhow!("no month to try");
        for month in months.into_iter().flatten() {
            let url = format!(
                "https://download.db-ip.com/free/dbip-country-lite-{}.mmdb.gz",
                month.format("%Y-%m")
            );
            match fetch(http, &url).await {
                Ok(bytes) => {
                    let reader = Reader::from_source(bytes.clone())
                        .context("the downloaded database does not open")?;
                    crate::store::write_atomic(&self.path, &bytes)
                        .with_context(|| format!("cannot write {:?}", self.path))?;
                    return Ok(reader);
                }
                Err(err) => failure = err.context(url),
            }
        }
        Err(failure)
    }
}

async fn fetch(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let gzipped = http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(gzipped.as_ref())
            .read_to_end(&mut bytes)
            .context("the download is not a gzip file")?;
        Ok(bytes)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_database_finds_no_country() {
        let dir = tempfile::tempdir().unwrap();
        let countries = Countries::load(dir.path().join("none.mmdb"));
        assert!(countries.is_stale());
        assert_eq!(countries.of("1.1.1.1:6881"), None);
        assert_eq!(countries.of("not an address"), None);
    }

    #[tokio::test]
    #[ignore = "downloads the database over the network"]
    async fn downloads_the_database_and_finds_countries() {
        let dir = tempfile::tempdir().unwrap();
        let countries = Countries::load(dir.path().join("countries.mmdb"));
        countries.update(&reqwest::Client::new()).await;
        assert!(!countries.is_stale());
        assert_eq!(countries.of("8.8.8.8:53").as_deref(), Some("US"));
        assert_eq!(countries.of("[::ffff:8.8.8.8]:53").as_deref(), Some("US"));
        assert!(countries.of("[2001:4860:4860::8888]:53").is_some());
        assert_eq!(countries.of("192.168.1.2:6881"), None);
    }
}
