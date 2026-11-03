use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use bytes::Bytes;
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, DhtSessionConfig, ListenerMode,
    ListenerOptions, ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig,
    TorrentStatsState,
};
use tokio::sync::watch;

use crate::api::*;
use crate::magnet;
use crate::search;
use crate::settings::{Indexer, Settings};
use crate::store::{Entry, Store};

const HISTORY: usize = 300;
const PIECE_MAP: usize = 400;
const RESOLVED_TTL: Duration = Duration::from_secs(30 * 60);

type Handle = Arc<ManagedTorrent>;

struct Paths {
    settings: PathBuf,
    store: PathBuf,
    session: PathBuf,
    dht: PathBuf,
}

impl Paths {
    fn new(root: &Path) -> Self {
        Self {
            settings: root.join("settings.json"),
            store: root.join("torrents.json"),
            session: root.join("session"),
            dht: root.join("dht.json"),
        }
    }
}

/// A torrent whose metadata was fetched but which is not added yet.
struct Resolved {
    bytes: Bytes,
    name: String,
    size: u64,
    files: Vec<PreviewFile>,
    peers: Vec<SocketAddr>,
    at: Instant,
}

/// A magnet link still looking for its metadata, shown as a row until it arrives.
struct Pending {
    id: Id,
    name: String,
    added: i64,
    save_path: String,
    category: Option<String>,
    tags: Vec<String>,
    error: Option<String>,
}

#[derive(Default)]
struct Live {
    last_uploaded: HashMap<Id, u64>,
    peer_bytes: HashMap<(Id, String), (u64, u64, Instant)>,
    history: VecDeque<[u64; 2]>,
    scheduled: Option<bool>,
    applied_limits: Option<(u64, u64)>,
    free_space: Option<u64>,
    ticks: u64,
}

/// One running engine; cheap to clone and safe to share between the desktop app, the web server and the daemon.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

struct Inner {
    paths: Paths,
    session: RwLock<Arc<Session>>,
    settings: RwLock<Settings>,
    store: Mutex<Store>,
    store_dirty: AtomicBool,
    resolved: Mutex<HashMap<Id, Resolved>>,
    pending: Mutex<Vec<Pending>>,
    live: Mutex<Live>,
    snapshot: watch::Sender<Arc<Snapshot>>,
    rev: AtomicU64,
    rebuilding: tokio::sync::Mutex<()>,
    http: reqwest::Client,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

enum Op {
    Pause(Handle),
    Resume(Handle),
}

impl Core {
    /// Starts the engine on the current tokio runtime, with its files under `root`.
    pub async fn start(root: PathBuf) -> Result<Core> {
        std::fs::create_dir_all(&root).with_context(|| format!("cannot create {root:?}"))?;
        let paths = Paths::new(&root);
        let settings = Settings::load(&paths.settings).unwrap_or_default();
        settings
            .save(&paths.settings)
            .with_context(|| format!("cannot write {:?}", paths.settings))?;
        let store = Store::load(&paths.store);
        let session = build_session(&paths, &settings).await?;
        let (snapshot, _) = watch::channel(Arc::new(Snapshot::default()));

        let core = Core {
            inner: Arc::new(Inner {
                paths,
                session: RwLock::new(session),
                settings: RwLock::new(settings),
                store: Mutex::new(store),
                store_dirty: AtomicBool::new(false),
                resolved: Mutex::default(),
                pending: Mutex::default(),
                live: Mutex::default(),
                snapshot,
                rev: AtomicU64::new(0),
                rebuilding: tokio::sync::Mutex::new(()),
                http: search::client(),
            }),
        };
        core.tick().await;
        let ticker = core.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                ticker.tick().await;
            }
        });
        Ok(core)
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Snapshot>> {
        self.inner.snapshot.subscribe()
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.inner.snapshot.borrow().clone()
    }

    pub fn settings(&self) -> Settings {
        self.inner
            .settings
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn session(&self) -> Arc<Session> {
        self.inner
            .session
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn api(&self) -> Api {
        Api::new(self.session(), None)
    }

    /// Saves what is not saved yet and stops the engine.
    pub async fn shutdown(&self) {
        self.save_store();
        self.session().stop().await;
    }

    pub async fn update_settings(&self, incoming: Settings) -> Result<Settings> {
        let old = self.settings();
        let new = old.merge_from_client(incoming);
        if new.download_dir.trim().is_empty() {
            bail!("pick a download folder");
        }
        new.save(&self.inner.paths.settings)
            .context("cannot save the settings")?;
        *self
            .inner
            .settings
            .write()
            .unwrap_or_else(|p| p.into_inner()) = new.clone();
        if old.needs_new_session(&new) || old.download_dir != new.download_dir {
            self.rebuild_session().await?;
        }
        self.tick().await;
        Ok(new.public())
    }

    /// Sets the stored web password hash, which clients can never change through `update_settings`.
    pub fn set_web_password_hash(&self, hash: String) -> Result<()> {
        let mut settings = self
            .inner
            .settings
            .write()
            .unwrap_or_else(|p| p.into_inner());
        settings.web.password_hash = hash;
        settings
            .save(&self.inner.paths.settings)
            .context("cannot save the settings")
    }

    /// Restarts the engine with new network settings; the session folder brings every torrent back.
    async fn rebuild_session(&self) -> Result<()> {
        let _guard = self.inner.rebuilding.lock().await;
        let settings = self.settings();
        let old = self.session();
        old.stop().await;
        drop(old);
        let session = build_session(&self.inner.paths, &settings).await?;
        *self
            .inner
            .session
            .write()
            .unwrap_or_else(|p| p.into_inner()) = session;
        lock(&self.inner.live).applied_limits = None;
        Ok(())
    }

    fn handles(&self) -> Vec<Handle> {
        self.session()
            .with_torrents(|torrents| torrents.map(|(_, handle)| handle.clone()).collect())
    }

    fn handle(&self, id: &str) -> Result<Handle> {
        self.session()
            .get(TorrentIdOrHash::Hash(id20(id)?))
            .ok_or_else(|| anyhow!("no torrent with id {id}"))
    }

    fn save_store(&self) {
        if self.inner.store_dirty.swap(false, Ordering::Relaxed) {
            let store = lock(&self.inner.store).clone();
            if let Err(err) = store.save(&self.inner.paths.store) {
                tracing::warn!("cannot save torrents.json: {err}");
                self.inner.store_dirty.store(true, Ordering::Relaxed);
            }
        }
    }

    fn edit_store<R>(&self, edit: impl FnOnce(&mut Store) -> R) -> R {
        let result = edit(&mut lock(&self.inner.store));
        self.inner.store_dirty.store(true, Ordering::Relaxed);
        self.save_store();
        result
    }

    pub async fn execute(&self, action: Action) -> Result<Reply> {
        match action {
            Action::Pause { ids } => {
                for id in ids {
                    if let Ok(handle) = self.handle(&id) {
                        self.session().pause(&handle).await.ok();
                    }
                    self.edit_store(|store| store.entry(&id).queued = false);
                }
            }
            Action::Resume { ids } => {
                for id in ids {
                    let Ok(handle) = self.handle(&id) else {
                        continue;
                    };
                    let finished = handle.stats().finished;
                    self.edit_store(|store| {
                        let entry = store.entry(&id);
                        entry.queued = false;
                        if entry.done || finished {
                            entry.keep_seeding = entry.done || entry.keep_seeding;
                            entry.done = false;
                        }
                    });
                    self.session().unpause(&handle).await.ok();
                }
            }
            Action::Remove { ids, delete_files } => {
                for id in ids {
                    lock(&self.inner.pending).retain(|pending| pending.id != id);
                    if let Ok(hash) = id20(&id)
                        && self.session().get(TorrentIdOrHash::Hash(hash)).is_some()
                    {
                        self.session()
                            .delete(TorrentIdOrHash::Hash(hash), delete_files)
                            .await?;
                    }
                    self.edit_store(|store| store.torrents.remove(&id));
                }
            }
            Action::SetFiles { id, files } => {
                let handle = self.handle(&id)?;
                let files: HashSet<usize> = files.into_iter().collect();
                if files.is_empty() {
                    bail!("pick at least one file");
                }
                self.session().update_only_files(&handle, &files).await?;
            }
            Action::SetCategory { ids, category } => {
                let category = category.filter(|name| !name.trim().is_empty());
                self.edit_store(|store| {
                    if let Some(name) = &category {
                        store.categories.entry(name.clone()).or_default();
                    }
                    for id in &ids {
                        store.entry(id).category = category.clone();
                    }
                });
            }
            Action::AddTags { ids, tags } => {
                let tags = clean_tags(tags);
                self.edit_store(|store| {
                    store.tags.extend(tags.iter().cloned());
                    for id in &ids {
                        let entry = store.entry(id);
                        for tag in &tags {
                            if !entry.tags.contains(tag) {
                                entry.tags.push(tag.clone());
                            }
                        }
                    }
                });
            }
            Action::RemoveTags { ids, tags } => self.edit_store(|store| {
                for id in &ids {
                    store.entry(id).tags.retain(|tag| !tags.contains(tag));
                }
            }),
            Action::SaveCategory { name, save_path } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    bail!("name the category");
                }
                self.edit_store(|store| {
                    store
                        .categories
                        .insert(name, save_path.unwrap_or_default().trim().to_string())
                });
            }
            Action::RemoveCategory { name } => self.edit_store(|store| {
                store.categories.remove(&name);
                for entry in store.torrents.values_mut() {
                    if entry.category.as_deref() == Some(name.as_str()) {
                        entry.category = None;
                    }
                }
            }),
            Action::RemoveTag { tag } => self.edit_store(|store| {
                store.tags.remove(&tag);
                for entry in store.torrents.values_mut() {
                    entry.tags.retain(|existing| *existing != tag);
                }
            }),
            Action::SetAltSpeed { on } => {
                let mut settings = self.settings();
                settings.alt_speed = on;
                settings.save(&self.inner.paths.settings)?;
                *self
                    .inner
                    .settings
                    .write()
                    .unwrap_or_else(|p| p.into_inner()) = settings;
            }
            Action::PauseAll => {
                let ids = self
                    .handles()
                    .iter()
                    .map(|h| h.info_hash().as_string())
                    .collect();
                return Box::pin(self.execute(Action::Pause { ids })).await;
            }
            Action::ResumeAll => {
                let ids = self
                    .snapshot()
                    .torrents
                    .iter()
                    .filter(|torrent| torrent.state == State::Paused)
                    .map(|torrent| torrent.id.clone())
                    .collect();
                return Box::pin(self.execute(Action::Resume { ids })).await;
            }
        }
        self.tick().await;
        Ok(Reply::Done)
    }

    /// Fetches a torrent's file list without adding it, for the add dialog.
    pub async fn preview(&self, source: Source) -> Result<Preview> {
        let settings = self.settings();
        let (id, resolved_existing) = match &source {
            Source::Resolved(id) => (id.clone(), true),
            _ => (self.resolve(source).await?, false),
        };
        let existing = self.handle(&id).is_ok();
        let resolved = lock(&self.inner.resolved);
        let Some(resolved) = resolved.get(&id) else {
            if resolved_existing {
                bail!("that torrent is no longer ready to add; add it again");
            }
            bail!("could not read that torrent");
        };
        Ok(Preview {
            id,
            name: resolved.name.clone(),
            size: resolved.size,
            files: resolved.files.clone(),
            save_path: settings.download_dir,
            existing,
        })
    }

    /// Gets a torrent's metadata into the resolved cache and returns its id.
    async fn resolve(&self, source: Source) -> Result<Id> {
        let source = match source {
            Source::Link(link) if is_web_link(&link) => {
                match search::fetch_link(&self.inner.http, &link).await? {
                    search::Fetched::Magnet(magnet) => Source::Link(magnet),
                    search::Fetched::Torrent(bytes) => Source::File(bytes),
                }
            }
            other => other,
        };
        if let Source::Link(link) = &source
            && let Some(info) = magnet::parse(link)
            && lock(&self.inner.resolved).contains_key(&info.id)
        {
            return Ok(info.id);
        }
        let add = match source {
            Source::Link(link) => AddTorrent::from_url(link.trim().to_string()),
            Source::File(bytes) => AddTorrent::from_bytes(bytes),
            Source::Resolved(id) => return Ok(id),
        };
        let options = AddTorrentOptions {
            list_only: true,
            ..Default::default()
        };
        let response = self
            .session()
            .add_torrent(add, Some(options))
            .await
            .map_err(|err| anyhow!("{err:#}"))?;
        let (id, resolved) = match response {
            AddTorrentResponse::ListOnly(listed) => {
                let id = listed.info_hash.as_string();
                let files: Vec<PreviewFile> = listed
                    .info
                    .iter_file_details()
                    .enumerate()
                    .map(|(index, details)| PreviewFile {
                        index,
                        path: details.filename.to_vec().join("/"),
                        size: details.len,
                    })
                    .collect();
                let name = listed
                    .info
                    .name()
                    .map(|name| name.into_owned())
                    .unwrap_or_else(|| id.clone());
                let resolved = Resolved {
                    bytes: listed.torrent_bytes,
                    name,
                    size: files.iter().map(|file| file.size).sum(),
                    files,
                    peers: listed.seen_peers,
                    at: Instant::now(),
                };
                (id, resolved)
            }
            AddTorrentResponse::AlreadyManaged(_, handle) => {
                let id = handle.info_hash().as_string();
                let resolved = resolved_from_handle(&handle)?;
                (id, resolved)
            }
            AddTorrentResponse::Added(..) => {
                bail!("the engine added a torrent it was asked to list")
            }
        };
        let mut cache = lock(&self.inner.resolved);
        cache.retain(|_, entry| entry.at.elapsed() < RESOLVED_TTL);
        cache.insert(id.clone(), resolved);
        Ok(id)
    }

    /// Adds a torrent. Magnet links return at once and show a row while their metadata arrives.
    pub async fn add(&self, request: AddRequest) -> Result<Id> {
        if let Source::Link(link) = &request.source
            && let Some(info) = magnet::parse(link)
        {
            if self.handle(&info.id).is_ok() {
                return Ok(info.id);
            }
            if !lock(&self.inner.resolved).contains_key(&info.id) {
                let id = info.id.clone();
                self.add_in_background(info, request);
                return Ok(id);
            }
        }
        let id = self.resolve(request.source.clone()).await?;
        self.add_resolved(&id, &request).await?;
        Ok(id)
    }

    fn add_in_background(&self, info: magnet::MagnetInfo, request: AddRequest) {
        {
            let mut pending = lock(&self.inner.pending);
            if pending.iter().any(|row| row.id == info.id) {
                return;
            }
            pending.push(Pending {
                id: info.id.clone(),
                name: info.name.clone().unwrap_or_else(|| info.id.clone()),
                added: crate::now(),
                save_path: self.save_path_for(&request),
                category: request.category.clone(),
                tags: request.tags.clone(),
                error: None,
            });
        }
        let core = self.clone();
        tokio::spawn(async move {
            let result = async {
                let id = core.resolve(request.source.clone()).await?;
                let still_wanted = lock(&core.inner.pending).iter().any(|row| row.id == id);
                if still_wanted {
                    core.add_resolved(&id, &request).await?;
                }
                anyhow::Ok(())
            }
            .await;
            let mut pending = lock(&core.inner.pending);
            match result {
                Ok(()) => pending.retain(|row| row.id != info.id),
                Err(err) => {
                    if let Some(row) = pending.iter_mut().find(|row| row.id == info.id) {
                        row.error = Some(format!("{err:#}"));
                    }
                }
            }
        });
    }

    fn save_path_for(&self, request: &AddRequest) -> String {
        if let Some(path) = request.save_path.as_ref().filter(|p| !p.trim().is_empty()) {
            return path.trim().to_string();
        }
        let settings = self.settings();
        let Some(category) = request.category.as_ref().filter(|c| !c.trim().is_empty()) else {
            return settings.download_dir;
        };
        let custom = lock(&self.inner.store)
            .categories
            .get(category)
            .filter(|path| !path.is_empty())
            .cloned();
        custom.unwrap_or_else(|| {
            Path::new(&settings.download_dir)
                .join(sanitize(category))
                .to_string_lossy()
                .into_owned()
        })
    }

    async fn add_resolved(&self, id: &str, request: &AddRequest) -> Result<()> {
        if self.handle(id).is_ok() {
            return Ok(());
        }
        let resolved = lock(&self.inner.resolved)
            .remove(id)
            .ok_or_else(|| anyhow!("that torrent is no longer ready to add; add it again"))?;
        let settings = self.settings();
        let base = PathBuf::from(self.save_path_for(request));
        let output = if resolved.files.len() > 1 {
            base.join(sanitize(&resolved.name))
        } else {
            base
        };
        let paused = request.paused.unwrap_or(settings.start_paused);
        let queued = !paused && self.queue_is_full(&settings);
        let only_files = request
            .files
            .clone()
            .filter(|files| files.len() < resolved.files.len());
        if only_files.as_ref().is_some_and(|files| files.is_empty()) {
            bail!("pick at least one file");
        }
        let options = AddTorrentOptions {
            paused: paused || queued,
            output_folder: Some(output.to_string_lossy().into_owned()),
            only_files,
            overwrite: true,
            initial_peers: Some(resolved.peers),
            ..Default::default()
        };
        self.session()
            .add_torrent(AddTorrent::from_bytes(resolved.bytes), Some(options))
            .await
            .map_err(|err| anyhow!("{err:#}"))?;
        let tags = clean_tags(request.tags.clone());
        self.edit_store(|store| {
            store.tags.extend(tags.iter().cloned());
            if let Some(category) = &request.category {
                store.categories.entry(category.clone()).or_default();
            }
            let entry = store.entry(id);
            *entry = Entry {
                category: request.category.clone(),
                tags,
                added: crate::now(),
                queued,
                ..Entry::default()
            };
        });
        self.tick().await;
        Ok(())
    }

    fn queue_is_full(&self, settings: &Settings) -> bool {
        let max = settings.max_active_downloads as usize;
        max > 0
            && self
                .snapshot()
                .torrents
                .iter()
                .filter(|t| {
                    matches!(
                        t.state,
                        State::Downloading | State::Stalled | State::Checking
                    )
                })
                .count()
                >= max
    }

    /// Asks every turned-on Prowlarr and Jackett at once.
    pub async fn search(&self, request: SearchRequest) -> Result<SearchReply> {
        if request.query.trim().is_empty() {
            bail!("type something to search for");
        }
        let indexers = self.settings().indexers;
        if !indexers.iter().any(|indexer| indexer.enabled) {
            bail!("add prowlarr or jackett under settings, search");
        }
        Ok(search::search(&self.inner.http, &indexers, &request).await)
    }

    /// Checks an indexer's address and key; a blank key means the stored one.
    pub async fn test_indexer(&self, mut indexer: Indexer) -> Result<String> {
        if indexer.api_key.trim().is_empty()
            && let Some(stored) = self.settings().indexer(&indexer.id)
        {
            indexer.api_key = stored.api_key.clone();
        }
        search::test(&self.inner.http, &indexer).await
    }

    pub fn details(&self, id: &str) -> Result<Details> {
        let handle = self.handle(id)?;
        let api = self.api();
        let key = TorrentIdOrHash::Hash(handle.info_hash());
        let listed = api
            .api_torrent_details(key)
            .map_err(|err| anyhow!("{err}"))?;
        let stats = handle.stats();
        let files = listed
            .files
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, file)| File {
                index,
                path: file.components.join("/"),
                size: file.length,
                done: stats.file_progress.get(index).copied().unwrap_or_default(),
                included: file.included,
            })
            .collect();
        let (piece_size, pieces) = handle
            .metadata
            .load()
            .as_ref()
            .map(|meta| {
                let lengths = meta.info.lengths();
                (lengths.default_piece_length(), lengths.total_pieces())
            })
            .unwrap_or_default();
        let piece_map = match api.api_dump_haves(key) {
            Ok((have, total)) => {
                let bits: Vec<bool> = (0..total as usize)
                    .map(|ix| have.get(ix).map(|bit| *bit).unwrap_or(false))
                    .collect();
                piece_map(&bits)
            }
            Err(_) => {
                let fraction = stats.progress_bytes as f64 / stats.total_bytes.max(1) as f64;
                vec![(fraction * 255.0) as u8; PIECE_MAP.min(pieces.max(1) as usize)]
            }
        };
        let peers = self.peers(id, &api, key);
        let mut trackers: Vec<String> = handle
            .shared()
            .trackers
            .iter()
            .map(|url| url.to_string())
            .collect();
        trackers.sort();
        let name = handle.name().unwrap_or_else(|| id.to_string());
        Ok(Details {
            id: id.to_string(),
            magnet: magnet_link(id, &name, &trackers),
            name,
            save_path: handle.output_folder().to_string_lossy().into_owned(),
            size: stats.total_bytes,
            piece_size,
            pieces,
            piece_map,
            files,
            peers,
            trackers,
        })
    }

    fn peers(&self, id: &str, api: &Api, key: TorrentIdOrHash) -> Vec<Peer> {
        let Ok(snapshot) = api.api_peer_stats(key, Default::default()) else {
            return Vec::new();
        };
        let now = Instant::now();
        let mut live = lock(&self.inner.live);
        let mut peers: Vec<Peer> = snapshot
            .peers
            .into_iter()
            .map(|(addr, stats)| {
                let fetched = stats.counters.fetched_bytes;
                let uploaded = stats.counters.uploaded_bytes;
                let key = (id.to_string(), addr.clone());
                let (down, up) = match live.peer_bytes.get(&key) {
                    Some((last_down, last_up, at)) => {
                        let seconds = now.duration_since(*at).as_secs_f64().max(0.5);
                        (
                            (fetched.saturating_sub(*last_down) as f64 / seconds) as u64,
                            (uploaded.saturating_sub(*last_up) as f64 / seconds) as u64,
                        )
                    }
                    None => (0, 0),
                };
                live.peer_bytes.insert(key, (fetched, uploaded, now));
                Peer {
                    addr,
                    client: stats.client_name,
                    transport: stats
                        .conn_kind
                        .map(|kind| format!("{kind:?}").to_lowercase()),
                    down,
                    up,
                    downloaded: fetched,
                    uploaded,
                }
            })
            .collect();
        live.peer_bytes
            .retain(|_, (_, _, at)| now.duration_since(*at) < Duration::from_secs(30));
        peers.sort_by(|a, b| {
            (b.down + b.up)
                .cmp(&(a.down + a.up))
                .then(a.addr.cmp(&b.addr))
        });
        peers
    }

    /// Reads every torrent, applies the queue, seeding goals and schedule, and publishes a snapshot.
    async fn tick(&self) {
        let handles = self.handles();
        let settings = self.settings();
        let (mut torrents, ops) = self.read_torrents(&handles, &settings);
        for op in ops {
            let session = self.session();
            let result = match op {
                Op::Pause(handle) => session.pause(&handle).await,
                Op::Resume(handle) => session.unpause(&handle).await,
            };
            if let Err(err) = result {
                tracing::debug!("queue change failed: {err:#}");
            }
        }
        self.apply_schedule_and_limits(&settings);
        self.scan_watch_dir(&settings).await;

        let pending: Vec<Torrent> = lock(&self.inner.pending)
            .iter()
            .filter(|row| !torrents.iter().any(|t| t.id == row.id))
            .map(pending_row)
            .collect();
        torrents.extend(pending);

        let totals = self.totals(&settings);
        let (categories, tags) = {
            let store = lock(&self.inner.store);
            let categories = store
                .categories
                .iter()
                .map(|(name, path)| Category {
                    name: name.clone(),
                    save_path: path.clone(),
                })
                .collect();
            (categories, store.all_tags())
        };
        let history = {
            let mut live = lock(&self.inner.live);
            live.history.push_back([totals.down, totals.up]);
            while live.history.len() > HISTORY {
                live.history.pop_front();
            }
            live.ticks += 1;
            live.history.iter().copied().collect()
        };
        let rev = self.inner.rev.fetch_add(1, Ordering::Relaxed) + 1;
        self.inner.snapshot.send_replace(Arc::new(Snapshot {
            rev,
            torrents,
            totals,
            categories,
            tags,
            history,
        }));
        if rev.is_multiple_of(10) {
            self.save_store();
        }
    }

    fn read_torrents(&self, handles: &[Handle], settings: &Settings) -> (Vec<Torrent>, Vec<Op>) {
        let now = crate::now();
        let mut store = lock(&self.inner.store);
        let mut live = lock(&self.inner.live);
        let mut dirty = false;
        let mut rows = Vec::with_capacity(handles.len());
        let mut ops = Vec::new();
        let mut queue: Vec<(i64, usize)> = Vec::new();

        for handle in handles {
            let id = handle.info_hash().as_string();
            let stats = handle.stats();
            if !store.torrents.contains_key(&id) {
                dirty = true;
            }
            let entry = store.entry(&id);

            let last = live
                .last_uploaded
                .insert(id.clone(), stats.uploaded_bytes)
                .unwrap_or(0);
            let sent = if stats.uploaded_bytes >= last {
                stats.uploaded_bytes - last
            } else {
                stats.uploaded_bytes
            };
            if sent > 0 {
                entry.uploaded += sent;
                dirty = true;
            }
            if stats.finished && entry.completed.is_none() {
                entry.completed = Some(now);
                dirty = true;
            }

            let (down, up, peers, seeds) = stats
                .live
                .as_ref()
                .map(|live| {
                    (
                        live.download_speed.as_bytes(),
                        live.upload_speed.as_bytes(),
                        live.snapshot.peer_stats.live,
                        live.snapshot.peer_stats.live_seeders,
                    )
                })
                .unwrap_or_default();
            let state = match stats.state {
                TorrentStatsState::Initializing { .. } => State::Checking,
                TorrentStatsState::Live if stats.finished => State::Seeding,
                TorrentStatsState::Live if down == 0 => State::Stalled,
                TorrentStatsState::Live => State::Downloading,
                TorrentStatsState::Paused if entry.queued && !stats.finished => State::Queued,
                TorrentStatsState::Paused if stats.finished => State::Done,
                TorrentStatsState::Paused => State::Paused,
                TorrentStatsState::Error => State::Error,
            };
            let size = selected_size(handle).unwrap_or(stats.total_bytes);
            let done = if stats.finished {
                size
            } else {
                stats.progress_bytes.min(size)
            };
            let remaining = size.saturating_sub(done);
            let eta = (down > 0 && !stats.finished).then(|| remaining / down.max(1));
            let ratio = entry.uploaded as f32 / done.max(1) as f32;

            if state == State::Seeding
                && !entry.keep_seeding
                && goal_reached(settings, ratio, entry.completed, now)
            {
                entry.done = true;
                dirty = true;
                ops.push(Op::Pause(handle.clone()));
            }
            if matches!(
                state,
                State::Downloading | State::Stalled | State::Queued | State::Checking
            ) {
                queue.push((entry.added, rows.len()));
            }

            rows.push(Torrent {
                name: handle.name().unwrap_or_else(|| id.clone()),
                state,
                progress: if size == 0 {
                    0.0
                } else {
                    (done as f64 / size as f64) as f32
                },
                size,
                done,
                uploaded: entry.uploaded,
                down,
                up,
                eta,
                ratio,
                peers,
                seeds,
                category: entry.category.clone(),
                tags: entry.tags.clone(),
                added: entry.added,
                completed: entry.completed,
                save_path: handle.output_folder().to_string_lossy().into_owned(),
                error: stats.error.clone(),
                id,
            });
        }

        queue.sort();
        let max = settings.max_active_downloads as usize;
        for (position, (_, row)) in queue.iter().enumerate() {
            let torrent = &mut rows[*row];
            let should_run = max == 0 || position < max;
            let handle = &handles[*row];
            match (torrent.state, should_run) {
                (State::Queued, true) => {
                    store.entry(&torrent.id).queued = false;
                    dirty = true;
                    ops.push(Op::Resume(handle.clone()));
                }
                (State::Downloading | State::Stalled, false) => {
                    store.entry(&torrent.id).queued = true;
                    dirty = true;
                    torrent.state = State::Queued;
                    ops.push(Op::Pause(handle.clone()));
                }
                _ => {}
            }
        }

        live.last_uploaded
            .retain(|id, _| rows.iter().any(|row| &row.id == id));
        if dirty {
            self.inner.store_dirty.store(true, Ordering::Relaxed);
        }
        (rows, ops)
    }

    fn apply_schedule_and_limits(&self, settings: &Settings) {
        let want = settings.schedule.covers_now();
        let mut live = lock(&self.inner.live);
        let edge = match live.scheduled {
            Some(previous) => previous != want,
            None => want,
        };
        live.scheduled = Some(want);
        let mut settings = settings.clone();
        if edge && settings.alt_speed != want {
            settings.alt_speed = want;
            if let Err(err) = settings.save(&self.inner.paths.settings) {
                tracing::warn!("cannot save the settings: {err}");
            }
            *self
                .inner
                .settings
                .write()
                .unwrap_or_else(|p| p.into_inner()) = settings.clone();
        }
        let limits = settings.effective_limits();
        if live.applied_limits != Some(limits) {
            let session = self.session();
            session.ratelimits.set_download_bps(bps(limits.0));
            session.ratelimits.set_upload_bps(bps(limits.1));
            live.applied_limits = Some(limits);
        }
    }

    async fn scan_watch_dir(&self, settings: &Settings) {
        let ticks = lock(&self.inner.live).ticks;
        if settings.watch_dir.trim().is_empty() || !ticks.is_multiple_of(5) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(settings.watch_dir.trim()) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_torrent = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("torrent"));
            if !is_torrent {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let request = AddRequest {
                source: Source::File(bytes),
                save_path: None,
                category: None,
                tags: Vec::new(),
                paused: None,
                files: None,
            };
            let mut done = path.clone().into_os_string();
            match Box::pin(self.add(request)).await {
                Ok(_) => done.push(".added"),
                Err(err) => {
                    tracing::warn!("cannot add {path:?} from the watch folder: {err:#}");
                    done.push(".invalid");
                }
            }
            let _ = std::fs::rename(&path, done);
        }
    }

    fn totals(&self, settings: &Settings) -> Totals {
        let session = self.session();
        let stats = Api::new(session.clone(), None).api_session_stats();
        let (down_limit, up_limit) = settings.effective_limits();
        let dht_nodes = session
            .get_dht()
            .map(|dht| {
                let stats = dht.stats();
                stats.routing_table_size + stats.routing_table_size_v6
            })
            .unwrap_or_default();
        let free_space = {
            let mut live = lock(&self.inner.live);
            if live.ticks.is_multiple_of(10) || live.free_space.is_none() {
                live.free_space = free_space(Path::new(&settings.download_dir));
            }
            live.free_space
        };
        Totals {
            down: stats.download_speed.as_bytes(),
            up: stats.upload_speed.as_bytes(),
            down_limit,
            up_limit,
            alt_speed: settings.alt_speed,
            dht_nodes,
            downloaded: stats.counters.fetched_bytes,
            uploaded: stats.counters.uploaded_bytes,
            free_space,
            port: session.announce_port(),
        }
    }
}

async fn build_session(paths: &Paths, settings: &Settings) -> Result<Arc<Session>> {
    let download_dir = PathBuf::from(&settings.download_dir);
    std::fs::create_dir_all(&download_dir).ok();
    let options = |listen: bool| {
        let (down, up) = settings.effective_limits();
        SessionOptions {
            dht: settings.dht.then(|| DhtSessionConfig {
                bootstrap_addrs: None,
                port: None,
                persistence: Some(DhtPersistenceConfig {
                    dump_interval: None,
                    config_filename: Some(paths.dht.clone()),
                }),
            }),
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(paths.session.clone()),
            }),
            peer_id: Some(peer_id()),
            listen: listen.then(|| ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: (Ipv6Addr::UNSPECIFIED, settings.listen_port).into(),
                enable_upnp_port_forwarding: settings.upnp,
                ..Default::default()
            }),
            ratelimits: LimitsConfig {
                download_bps: bps(down),
                upload_bps: bps(up),
            },
            peer_limit: (settings.peer_limit > 0).then_some(settings.peer_limit as usize),
            client_name_and_version: Some(format!("bawkbit {}", crate::VERSION)),
            ..Default::default()
        }
    };
    match Session::new_with_opts(download_dir.clone(), options(true)).await {
        Ok(session) => Ok(session),
        Err(err) => {
            tracing::warn!(
                "cannot listen on port {}: {err:#}; starting without incoming connections",
                settings.listen_port
            );
            Session::new_with_opts(download_dir, options(false))
                .await
                .context("cannot start the torrent engine")
        }
    }
}

/// An Azureus-style peer id, so other clients show "bawkbit" for us.
fn peer_id() -> Id20 {
    let mut id = *b"-BB0100-000000000000";
    let version: Vec<u8> = crate::VERSION
        .split('.')
        .map(|part| part.parse::<u8>().unwrap_or(0).min(9) + b'0')
        .collect();
    if let [major, minor, patch] = version[..] {
        id[3] = major;
        id[4] = minor;
        id[5] = patch;
    }
    let mut random = [0u8; 12];
    let _ = getrandom::fill(&mut random);
    const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    for (slot, byte) in id[8..].iter_mut().zip(random) {
        *slot = ALPHABET[byte as usize % ALPHABET.len()];
    }
    Id20::new(id)
}

fn is_web_link(link: &str) -> bool {
    let link = link.trim().to_ascii_lowercase();
    link.starts_with("http://") || link.starts_with("https://")
}

fn bps(limit: u64) -> Option<NonZeroU32> {
    NonZeroU32::new(limit.min(u32::MAX as u64) as u32)
}

fn id20(id: &str) -> Result<Id20> {
    Id20::from_str(id.trim()).map_err(|_| anyhow!("{id} is not a torrent id"))
}

fn goal_reached(settings: &Settings, ratio: f32, completed: Option<i64>, now: i64) -> bool {
    let by_ratio = settings.ratio_limit > 0.0 && ratio >= settings.ratio_limit;
    let by_time = settings.seed_minutes > 0
        && completed.is_some_and(|at| now - at >= i64::from(settings.seed_minutes) * 60);
    by_ratio || by_time
}

/// The bytes of the picked files; the engine counts whole pieces, which overshoots for a small file.
fn selected_size(handle: &Handle) -> Option<u64> {
    let only = handle.only_files()?;
    let metadata = handle.metadata.load();
    let lengths = metadata.as_ref()?.info.iter_file_lengths();
    Some(
        lengths
            .enumerate()
            .filter(|(index, _)| only.contains(index))
            .map(|(_, length)| length)
            .sum(),
    )
}

fn resolved_from_handle(handle: &Handle) -> Result<Resolved> {
    let metadata = handle.metadata.load();
    let metadata = metadata
        .as_ref()
        .context("that torrent has no metadata yet")?;
    let files: Vec<PreviewFile> = metadata
        .info
        .iter_file_details()
        .enumerate()
        .map(|(index, details)| PreviewFile {
            index,
            path: details.filename.to_vec().join("/"),
            size: details.len,
        })
        .collect();
    Ok(Resolved {
        bytes: metadata.torrent_bytes.clone(),
        name: handle
            .name()
            .unwrap_or_else(|| handle.info_hash().as_string()),
        size: files.iter().map(|file| file.size).sum(),
        files,
        peers: Vec::new(),
        at: Instant::now(),
    })
}

fn pending_row(pending: &Pending) -> Torrent {
    Torrent {
        id: pending.id.clone(),
        name: pending.name.clone(),
        state: if pending.error.is_some() {
            State::Error
        } else {
            State::Metadata
        },
        progress: 0.0,
        size: 0,
        done: 0,
        uploaded: 0,
        down: 0,
        up: 0,
        eta: None,
        ratio: 0.0,
        peers: 0,
        seeds: 0,
        category: pending.category.clone(),
        tags: pending.tags.clone(),
        added: pending.added,
        completed: None,
        save_path: pending.save_path.clone(),
        error: pending.error.clone(),
    }
}

/// Buckets of pieces, each 0 to 255 for how many of its pieces are here.
fn piece_map(have: &[bool]) -> Vec<u8> {
    if have.is_empty() {
        return Vec::new();
    }
    let buckets = PIECE_MAP.min(have.len());
    (0..buckets)
        .map(|bucket| {
            let start = bucket * have.len() / buckets;
            let end = ((bucket + 1) * have.len() / buckets).max(start + 1);
            let count = have[start..end].iter().filter(|bit| **bit).count();
            (count * 255 / (end - start)) as u8
        })
        .collect()
}

fn magnet_link(id: &str, name: &str, trackers: &[String]) -> String {
    let mut link = format!("magnet:?xt=urn:btih:{id}&dn={}", encode(name));
    for tracker in trackers {
        link.push_str("&tr=");
        link.push_str(&encode(tracker));
    }
    link
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn clean_tags(tags: Vec<String>) -> Vec<String> {
    let mut clean: Vec<String> = tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    clean.sort();
    clean.dedup();
    clean
}

/// A torrent name as a folder name every platform accepts.
fn sanitize(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let clean = clean.trim().trim_end_matches('.').to_string();
    if clean.is_empty() {
        "torrent".into()
    } else {
        clean
    }
}

fn free_space(path: &Path) -> Option<u64> {
    let mut path = path.to_path_buf();
    while !path.exists() {
        path = path.parent()?.to_path_buf();
    }
    fs4::available_space(&path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piece_map_scales_to_buckets() {
        let have: Vec<bool> = (0..800).map(|ix| ix < 400).collect();
        let map = piece_map(&have);
        assert_eq!(map.len(), PIECE_MAP);
        assert_eq!(map[0], 255);
        assert_eq!(map[PIECE_MAP - 1], 0);
        assert_eq!(piece_map(&[true, false]), vec![255, 0]);
    }

    #[test]
    fn sanitizes_folder_names() {
        assert_eq!(sanitize("a/b:c?"), "a_b_c_");
        assert_eq!(sanitize(" name. "), "name");
        assert_eq!(sanitize("..."), "torrent");
    }

    #[test]
    fn seeding_goals() {
        let mut settings = Settings::default();
        assert!(!goal_reached(&settings, 5.0, Some(0), 1_000_000));
        settings.ratio_limit = 2.0;
        assert!(goal_reached(&settings, 2.0, None, 0));
        assert!(!goal_reached(&settings, 1.9, None, 0));
        settings.seed_minutes = 60;
        assert!(goal_reached(&settings, 0.0, Some(0), 3600));
        assert!(!goal_reached(&settings, 0.0, Some(0), 3599));
    }

    #[test]
    fn peer_ids_name_the_version() {
        let digits: String = crate::VERSION.split('.').collect();
        let prefix = format!("-BB{digits}0-");
        let hex: String = prefix.bytes().map(|b| format!("{b:02x}")).collect();
        assert!(peer_id().as_string().starts_with(&hex));
    }

    #[test]
    fn magnet_links_escape_names() {
        let link = magnet_link("ab", "a b&c", &["udp://t:1".into()]);
        assert_eq!(
            link,
            "magnet:?xt=urn:btih:ab&dn=a%20b%26c&tr=udp%3A%2F%2Ft%3A1"
        );
    }
}
