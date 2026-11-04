use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use bytes::Bytes;
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::limits::{Limits, LimitsConfig};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, DhtSessionConfig, ListenerMode,
    ListenerOptions, ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig,
    TorrentStatsState,
};
use tokio::sync::{broadcast, watch};

use crate::api::*;
use crate::magnet;
use crate::rss::Feeds;
use crate::search;
use crate::settings::{Indexer, Settings};
use crate::store::{Entry, Store, Waiting};

const HISTORY: usize = 300;
const PIECE_MAP: usize = 400;
const RESOLVED_TTL: Duration = Duration::from_secs(30 * 60);

type Handle = Arc<ManagedTorrent>;

struct Paths {
    settings: PathBuf,
    store: PathBuf,
    session: PathBuf,
    dht: PathBuf,
    rss: PathBuf,
}

impl Paths {
    fn new(root: &Path) -> Self {
        Self {
            settings: root.join("settings.json"),
            store: root.join("torrents.json"),
            session: root.join("session"),
            dht: root.join("dht.json"),
            rss: root.join("rss.json"),
        }
    }
}

/// A torrent that just finished, with what the finish program is told about it.
struct Done {
    id: Id,
    name: String,
    category: Option<String>,
    tags: Vec<String>,
    /// The file, or the folder of a torrent with several.
    content: PathBuf,
    /// The folder that holds `content`.
    folder: PathBuf,
    size: u64,
    files: usize,
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
    /// Torrents seen downloading, so one counts as finished only when it gets there while bawkbit watches.
    downloading: HashSet<Id>,
    /// The limits each limited category's torrents share.
    groups: HashMap<String, Arc<Limits>>,
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
    feeds: Feeds,
    finished: broadcast::Sender<Finished>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

enum Op {
    Pause(Handle),
    Resume(Handle),
    /// Drops every peer so the next ones hear whether this torrent uploads.
    Reconnect(Handle),
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
        let feeds = Feeds::load(paths.rss.clone());

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
                feeds,
                finished: broadcast::channel(64).0,
            }),
        };
        core.tick().await;
        core.resume_waiting();
        let ticker = core.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                ticker.tick().await;
            }
        });
        let reader = core.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let minutes = reader.settings().rss_minutes;
                for id in reader.inner.feeds.due(minutes) {
                    reader.spawn_refresh(id);
                }
            }
        });
        Ok(core)
    }

    /// Picks up the magnet links that were still fetching their file list when bawkbit last stopped.
    fn resume_waiting(&self) {
        let waiting = lock(&self.inner.store).waiting.clone();
        for (id, waiting) in waiting {
            let info = match &waiting.request.source {
                Source::Link(link) => magnet::parse(link),
                _ => None,
            };
            match info {
                Some(info) if self.handle(&id).is_err() => {
                    self.add_in_background(info, waiting.request, waiting.added)
                }
                _ => {
                    self.edit_store(|store| store.waiting.remove(&id));
                }
            }
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Snapshot>> {
        self.inner.snapshot.subscribe()
    }

    /// Each torrent that finishes downloading from now on.
    pub fn subscribe_finished(&self) -> broadcast::Receiver<Finished> {
        self.inner.finished.subscribe()
    }

    /// Sets the program run when a torrent finishes, which clients cannot change through `update_settings`.
    pub fn set_on_finish(&self, command: String) -> Result<()> {
        let mut settings = self
            .inner
            .settings
            .write()
            .unwrap_or_else(|p| p.into_inner());
        settings.on_finish = command.trim().to_string();
        settings
            .save(&self.inner.paths.settings)
            .context("cannot save the settings")
    }

    pub fn rss(&self) -> Rss {
        self.inner.feeds.get()
    }

    pub async fn rss_execute(&self, action: RssAction) -> Result<Rss> {
        let feeds = &self.inner.feeds;
        match action {
            RssAction::AddFeed { url } => {
                let id = feeds.add_feed(&url)?;
                self.spawn_refresh(id);
            }
            RssAction::UpdateFeed { id, name, enabled } => feeds.update_feed(&id, name, enabled)?,
            RssAction::RemoveFeed { id } => feeds.remove_feed(&id),
            RssAction::Refresh { id } => {
                for id in id.map(|id| vec![id]).unwrap_or_else(|| feeds.ids()) {
                    self.spawn_refresh(id);
                }
            }
            RssAction::SaveRule { rule } => {
                feeds.save_rule(rule)?;
            }
            RssAction::RemoveRule { id } => feeds.remove_rule(&id),
        }
        self.tick().await;
        Ok(feeds.get())
    }

    /// The feed items `rule` would take, for checking a rule before saving it.
    pub fn rule_matches(&self, rule: &Rule) -> Result<Vec<RuleMatch>> {
        self.inner.feeds.matches(rule)
    }

    fn spawn_refresh(&self, id: String) {
        let core = self.clone();
        tokio::spawn(async move { core.refresh_feed(&id).await });
    }

    async fn refresh_feed(&self, id: &str) {
        let takes = self.inner.feeds.refresh(id).await;
        for take in takes {
            let request = AddRequest {
                category: take.category,
                save_path: take.save_path,
                paused: take.paused.then_some(true),
                ..AddRequest::link(take.link)
            };
            if let Err(err) = Box::pin(self.add(request)).await {
                let error = format!("cannot add {}: {err:#}", take.title);
                tracing::warn!("{error}");
                self.inner.feeds.note_error(&take.feed, error);
            }
        }
        self.tick().await;
    }

    /// Settings, categories, tags and RSS, leaving out the web password and API keys.
    pub fn backup(&self) -> Backup {
        let (categories, tags) = {
            let store = lock(&self.inner.store);
            (categories(&store), store.all_tags())
        };
        let rss = self.inner.feeds.get();
        let feeds = rss
            .feeds
            .into_iter()
            .map(|feed| Feed {
                items: Vec::new(),
                checked: None,
                error: None,
                ready: false,
                ..feed
            })
            .collect();
        Backup {
            app: "bawkbit".into(),
            version: crate::VERSION.into(),
            settings: self.settings().public(),
            categories,
            tags,
            feeds,
            rules: rss.rules,
        }
    }

    /// Takes a backup's settings and adds its categories, tags, feeds and rules to those here.
    pub async fn restore(&self, backup: Backup) -> Result<()> {
        if backup.app != "bawkbit" {
            bail!("that file is not a bawkbit backup");
        }
        self.update_settings(backup.settings).await?;
        self.edit_store(|store| {
            for category in backup.categories {
                let limits = (category.down_limit, category.up_limit);
                if limits == (0, 0) {
                    store.category_limits.remove(&category.name);
                } else {
                    store.category_limits.insert(category.name.clone(), limits);
                }
                store.categories.insert(category.name, category.save_path);
            }
            store.tags.extend(clean_tags(backup.tags));
        });
        self.inner.feeds.restore(backup.feeds, backup.rules);
        self.tick().await;
        Ok(())
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
                    self.edit_store(|store| {
                        let entry = store.entry(&id);
                        entry.queued = false;
                        entry.forced = false;
                    });
                }
            }
            Action::Resume { ids } => self.resume(ids, false).await,
            Action::ForceStart { ids } => self.resume(ids, true).await,
            Action::Recheck { ids } => {
                for id in ids {
                    let trackers = self
                        .handle(&id)?
                        .shared()
                        .trackers
                        .iter()
                        .map(|url| url.to_string())
                        .collect();
                    self.reload(&id, trackers, false).await?;
                }
            }
            Action::SetTrackers { id, trackers } => {
                let trackers = clean_trackers(trackers)?;
                self.reload(&id, trackers, true).await?;
            }
            Action::SetNoUpload { ids, on } => self.edit_store(|store| {
                for id in ids {
                    store.entry(&id).no_upload = on;
                }
            }),
            Action::SetLimits { ids, down, up } => self.edit_store(|store| {
                for id in ids {
                    let entry = store.entry(&id);
                    entry.down_limit = down;
                    entry.up_limit = up;
                }
            }),
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
                    self.edit_store(|store| {
                        store.torrents.remove(&id);
                        store.waiting.remove(&id);
                    });
                    lock(&self.inner.live).downloading.remove(&id);
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
                store.category_limits.remove(&name);
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
            Action::RenameCategory { from, to } => {
                let to = to.trim().to_string();
                if to.is_empty() {
                    bail!("name the category");
                }
                if to != from {
                    self.edit_store(|store| {
                        if store.categories.contains_key(&to) {
                            bail!("there is already a category named {to}");
                        }
                        let path = store
                            .categories
                            .remove(&from)
                            .ok_or_else(|| anyhow!("no category named {from}"))?;
                        store.categories.insert(to.clone(), path);
                        if let Some(limits) = store.category_limits.remove(&from) {
                            store.category_limits.insert(to.clone(), limits);
                        }
                        for entry in store.torrents.values_mut() {
                            if entry.category.as_deref() == Some(from.as_str()) {
                                entry.category = Some(to.clone());
                            }
                        }
                        for waiting in store.waiting.values_mut() {
                            if waiting.request.category.as_deref() == Some(from.as_str()) {
                                waiting.request.category = Some(to.clone());
                            }
                        }
                        anyhow::Ok(())
                    })?;
                    for row in lock(&self.inner.pending).iter_mut() {
                        if row.category.as_deref() == Some(from.as_str()) {
                            row.category = Some(to.clone());
                        }
                    }
                    self.inner.feeds.rename_category(&from, &to);
                }
            }
            Action::RenameTag { from, to } => {
                let to = to.trim().to_string();
                if to.is_empty() {
                    bail!("name the tag");
                }
                self.edit_store(|store| {
                    store.tags.remove(&from);
                    store.tags.insert(to.clone());
                    for entry in store.torrents.values_mut() {
                        if entry.tags.contains(&from) {
                            entry.tags.retain(|tag| *tag != from && *tag != to);
                            entry.tags.push(to.clone());
                            entry.tags.sort();
                        }
                    }
                });
            }
            Action::SetCategoryLimits { name, down, up } => self.edit_store(|store| {
                if !store.categories.contains_key(&name) {
                    bail!("no category named {name}");
                }
                if (down, up) == (0, 0) {
                    store.category_limits.remove(&name);
                } else {
                    store.category_limits.insert(name, (down, up));
                }
                anyhow::Ok(())
            })?,
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

    async fn resume(&self, ids: Vec<Id>, forced: bool) {
        for id in ids {
            let Ok(handle) = self.handle(&id) else {
                continue;
            };
            let finished = handle.stats().finished;
            self.edit_store(|store| {
                let entry = store.entry(&id);
                entry.queued = false;
                entry.forced = forced;
                if entry.done || finished {
                    entry.keep_seeding = entry.done || entry.keep_seeding;
                    entry.done = false;
                }
            });
            self.session().unpause(&handle).await.ok();
        }
    }

    /// Takes a torrent out of the engine and adds it back with `trackers`, the only way the engine changes them; `keep_pieces` puts its saved piece map back, so only a spot check runs.
    async fn reload(&self, id: &str, trackers: Vec<String>, keep_pieces: bool) -> Result<()> {
        let _guard = self.inner.rebuilding.lock().await;
        let session = self.session();
        let handle = self.handle(id)?;
        let metadata = handle
            .metadata
            .load_full()
            .ok_or_else(|| anyhow!("wait until it has found its files"))?;
        let hash = handle.info_hash();
        let paused = handle.is_paused();
        let before: Vec<String> = handle
            .shared()
            .trackers
            .iter()
            .map(|url| url.to_string())
            .collect();
        let output = handle.output_folder().to_string_lossy().into_owned();
        let only_files = handle.only_files();
        let options = || AddTorrentOptions {
            paused,
            output_folder: Some(output.clone()),
            only_files: only_files.clone(),
            overwrite: true,
            ..Default::default()
        };
        if !paused {
            session.pause(&handle).await.ok();
        }
        drop(handle);

        let pieces = self.inner.paths.session.join(format!("{hash:?}.bitv"));
        let kept = pieces.with_extension("bitv.kept");
        if keep_pieces {
            tokio::fs::copy(&pieces, &kept).await.ok();
        }
        session.delete(TorrentIdOrHash::Hash(hash), false).await?;
        if keep_pieces {
            tokio::fs::rename(&kept, &pieces).await.ok();
        } else if let Err(err) = tokio::fs::remove_file(&pieces).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("cannot clear the saved pieces of {id}: {err}");
        }

        let add = |trackers: &[String]| {
            let bytes = Bytes::from(torrent_file(&metadata.info_bytes, trackers));
            session.add_torrent(AddTorrent::from_bytes(bytes), Some(options()))
        };
        let added = match add(&trackers).await {
            Ok(added) => added,
            Err(err) => {
                add(&before).await.map_err(|err| anyhow!("{err:#}"))?;
                bail!("{err:#}");
            }
        };
        if let Some(handle) = added.into_handle() {
            let entry = lock(&self.inner.store)
                .torrents
                .get(id)
                .cloned()
                .unwrap_or_default();
            let group = lock(&self.inner.live)
                .groups
                .get(entry.category.as_deref().unwrap_or_default())
                .cloned();
            apply_entry(&handle, &entry, group);
        }
        Ok(())
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
                self.add_in_background(info, request, crate::now());
                return Ok(id);
            }
        }
        let id = self.resolve(request.source.clone()).await?;
        self.add_resolved(&id, &request).await?;
        Ok(id)
    }

    fn add_in_background(&self, info: magnet::MagnetInfo, request: AddRequest, added: i64) {
        {
            let mut pending = lock(&self.inner.pending);
            if pending.iter().any(|row| row.id == info.id) {
                return;
            }
            pending.push(Pending {
                id: info.id.clone(),
                name: info.name.clone().unwrap_or_else(|| info.id.clone()),
                added,
                save_path: self.save_path_for(&request),
                category: request.category.clone(),
                tags: request.tags.clone(),
                error: None,
            });
        }
        let waiting = Waiting {
            added,
            request: request.clone(),
        };
        self.edit_store(|store| store.waiting.insert(info.id.clone(), waiting));
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
            if result.is_ok() {
                core.edit_store(|store| store.waiting.remove(&info.id));
            }
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
        if request.skip_check {
            let pieces = librqbit::torrent_from_bytes(&resolved.bytes)
                .map_err(|err| anyhow!("{err:#}"))?
                .info
                .data
                .pieces
                .as_ref()
                .len()
                / 20;
            let path = self
                .inner
                .paths
                .session
                .join(format!("{:?}.bitv", id20(id)?));
            std::fs::create_dir_all(&self.inner.paths.session).ok();
            std::fs::write(&path, full_bitfield(pieces))
                .with_context(|| format!("cannot write {path:?}"))?;
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
        if !request.skip_check {
            lock(&self.inner.live).downloading.insert(id.to_string());
        }
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
                    !t.forced
                        && matches!(
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
        if self.inner.rebuilding.try_lock().is_err() {
            return;
        }
        let handles = self.handles();
        let settings = self.settings();
        let (mut torrents, ops, finished) = self.read_torrents(&handles, &settings);
        for done in finished {
            run_on_finish(&settings.on_finish, &done);
            let _ = self.inner.finished.send(Finished {
                id: done.id,
                name: done.name,
            });
        }
        for op in ops {
            let session = self.session();
            let result = match op {
                Op::Pause(handle) => session.pause(&handle).await,
                Op::Resume(handle) => session.unpause(&handle).await,
                Op::Reconnect(handle) => match session.pause(&handle).await {
                    Ok(()) => session.unpause(&handle).await,
                    Err(err) => Err(err),
                },
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
            (categories(&store), store.all_tags())
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
            rss_rev: self.inner.feeds.rev(),
        }));
        if rev.is_multiple_of(10) {
            self.save_store();
        }
    }

    fn read_torrents(
        &self,
        handles: &[Handle],
        settings: &Settings,
    ) -> (Vec<Torrent>, Vec<Op>, Vec<Done>) {
        let now = crate::now();
        let mut store = lock(&self.inner.store);
        let mut live = lock(&self.inner.live);
        let mut dirty = false;
        let mut rows = Vec::with_capacity(handles.len());
        let mut ops = Vec::new();
        let mut finished = Vec::new();
        let mut queue: Vec<(i64, usize)> = Vec::new();

        let limited = &store.category_limits;
        live.groups
            .retain(|name, _| limited.get(name).is_some_and(|limits| *limits != (0, 0)));
        for (name, (down, up)) in limited.iter().filter(|(_, limits)| **limits != (0, 0)) {
            let group = live
                .groups
                .entry(name.clone())
                .or_insert_with(|| Arc::new(Limits::new(LimitsConfig::default())));
            if group.get_download_bps() != bps(*down) {
                group.set_download_bps(bps(*down));
            }
            if group.get_upload_bps() != bps(*up) {
                group.set_upload_bps(bps(*up));
            }
        }

        for handle in handles {
            let id = handle.info_hash().as_string();
            let stats = handle.stats();
            if !store.torrents.contains_key(&id) {
                dirty = true;
            }
            let entry = store.entry(&id);
            let group = entry
                .category
                .as_ref()
                .and_then(|name| live.groups.get(name))
                .cloned();
            if apply_entry(handle, entry, group) && handle.live().is_some() {
                ops.push(Op::Reconnect(handle.clone()));
            }

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

            let checked = matches!(
                stats.state,
                TorrentStatsState::Live | TorrentStatsState::Paused
            );
            if checked && !stats.finished {
                live.downloading.insert(id.clone());
            } else if stats.finished && live.downloading.remove(&id) {
                finished.push(done_info(handle, &id, entry, size));
            }

            if state == State::Seeding
                && !entry.keep_seeding
                && goal_reached(settings, ratio, entry.completed, now)
            {
                entry.done = true;
                dirty = true;
                ops.push(Op::Pause(handle.clone()));
            }
            if !entry.forced
                && matches!(
                    state,
                    State::Downloading | State::Stalled | State::Queued | State::Checking
                )
            {
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
                forced: entry.forced,
                no_upload: entry.no_upload,
                down_limit: entry.down_limit,
                up_limit: entry.up_limit,
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
        (rows, ops, finished)
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
                skip_check: false,
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
                mode: if settings.utp {
                    ListenerMode::TcpAndUtp
                } else {
                    ListenerMode::TcpOnly
                },
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
        forced: false,
        no_upload: false,
        down_limit: 0,
        up_limit: 0,
    }
}

/// Puts bawkbit's choices for one torrent into the engine; true when its upload switch changed.
fn apply_entry(handle: &Handle, entry: &Entry, group: Option<Arc<Limits>>) -> bool {
    let limits = handle.ratelimits();
    if limits.get_download_bps() != bps(entry.down_limit) {
        limits.set_download_bps(bps(entry.down_limit));
    }
    if limits.get_upload_bps() != bps(entry.up_limit) {
        limits.set_upload_bps(bps(entry.up_limit));
    }
    let same = match (handle.group_ratelimits(), &group) {
        (Some(current), Some(wanted)) => Arc::ptr_eq(&current, wanted),
        (current, wanted) => current.is_none() && wanted.is_none(),
    };
    if !same {
        handle.set_group_ratelimits(group);
    }
    handle.set_upload_disabled(entry.no_upload)
}

fn categories(store: &Store) -> Vec<Category> {
    store
        .categories
        .iter()
        .map(|(name, path)| {
            let (down_limit, up_limit) =
                store.category_limits.get(name).copied().unwrap_or_default();
            Category {
                name: name.clone(),
                save_path: path.clone(),
                down_limit,
                up_limit,
            }
        })
        .collect()
}

/// A piece map that claims every piece, in the engine's on-disk layout: one bit a piece, first piece in the top bit.
fn full_bitfield(pieces: usize) -> Vec<u8> {
    let mut bytes = vec![0xff; pieces.div_ceil(8)];
    if let Some(last) = bytes.last_mut()
        && !pieces.is_multiple_of(8)
    {
        *last = 0xff << (8 - pieces % 8);
    }
    bytes
}

fn done_info(handle: &Handle, id: &str, entry: &Entry, size: u64) -> Done {
    let output = handle.output_folder().to_path_buf();
    let metadata = handle.metadata.load();
    let files: Vec<PathBuf> = metadata
        .as_ref()
        .map(|meta| {
            meta.info
                .iter_file_details()
                .map(|details| details.filename.to_pathbuf())
                .collect()
        })
        .unwrap_or_default();
    let (content, folder) = match files.as_slice() {
        [only] => (output.join(only), output.clone()),
        _ => (
            output.clone(),
            output.parent().map(Path::to_path_buf).unwrap_or(output),
        ),
    };
    Done {
        id: id.to_string(),
        name: handle.name().unwrap_or_else(|| id.to_string()),
        category: entry.category.clone(),
        tags: entry.tags.clone(),
        content,
        folder,
        size,
        files: files.len(),
    }
}

/// Runs the finish program without a shell, so a torrent name can never become a command.
fn run_on_finish(command: &str, done: &Done) {
    let words = split_command(command);
    let Some((program, args)) = words.split_first() else {
        return;
    };
    let tags = done.tags.join(",");
    let category = done.category.clone().unwrap_or_default();
    let values = [
        ('N', done.name.clone()),
        ('F', done.content.to_string_lossy().into_owned()),
        ('D', done.folder.to_string_lossy().into_owned()),
        ('L', category.clone()),
        ('G', tags.clone()),
        ('Z', done.size.to_string()),
        ('C', done.files.to_string()),
        ('I', done.id.clone()),
    ];
    let fill = |word: &str| fill_placeholders(word, &values);
    let mut child = tokio::process::Command::new(fill(program));
    child
        .args(args.iter().map(|arg| fill(arg)))
        .env("BAWKBIT_NAME", &done.name)
        .env("BAWKBIT_CONTENT", &done.content)
        .env("BAWKBIT_FOLDER", &done.folder)
        .env("BAWKBIT_CATEGORY", category)
        .env("BAWKBIT_TAGS", tags)
        .env("BAWKBIT_SIZE", done.size.to_string())
        .env("BAWKBIT_ID", &done.id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    child.creation_flags(0x0800_0000);
    let program = program.clone();
    match child.spawn() {
        Ok(mut child) => {
            tokio::spawn(async move {
                match child.wait().await {
                    Ok(status) if !status.success() => {
                        tracing::warn!("the finish program {program} ended with {status}");
                    }
                    Err(err) => tracing::warn!("the finish program {program} failed: {err}"),
                    Ok(_) => {}
                }
            });
        }
        Err(err) => tracing::warn!("cannot run the finish program {program}: {err}"),
    }
}

/// Words split on spaces, with single or double quotes keeping spaces in; backslashes stay as typed for Windows paths.
fn split_command(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => word.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            None => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// qBittorrent's %N-style placeholders, so scripts written for it work here.
fn fill_placeholders(word: &str, values: &[(char, String)]) -> String {
    let mut out = String::with_capacity(word.len());
    let mut chars = word.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some(key) => match values.iter().find(|(name, _)| *name == key) {
                Some((_, value)) => {
                    chars.next();
                    out.push_str(value);
                }
                None => out.push('%'),
            },
            None => out.push('%'),
        }
    }
    out
}

/// Trimmed, without repeats, each an http, https or udp address.
fn clean_trackers(trackers: Vec<String>) -> Result<Vec<String>> {
    let mut clean: Vec<String> = Vec::new();
    for tracker in trackers.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        let url = reqwest::Url::parse(tracker)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https" | "udp") && url.has_host())
            .ok_or_else(|| anyhow!("{tracker} is not a tracker address"))?
            .to_string();
        if !clean.contains(&url) {
            clean.push(url);
        }
    }
    Ok(clean)
}

/// A .torrent of `info` as it is with `trackers` as its announce list, which the engine saves and restores.
fn torrent_file(info: &[u8], trackers: &[String]) -> Vec<u8> {
    fn text(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(format!("{}:", value.len()).as_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    let mut out = b"d".to_vec();
    if let Some(first) = trackers.first() {
        text(&mut out, "announce");
        text(&mut out, first);
        text(&mut out, "announce-list");
        out.push(b'l');
        for tracker in trackers {
            out.push(b'l');
            text(&mut out, tracker);
            out.push(b'e');
        }
        out.push(b'e');
    }
    text(&mut out, "info");
    out.extend_from_slice(info);
    out.push(b'e');
    out
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
    fn torrent_files_carry_the_trackers_and_keep_the_info_hash() {
        let info = b"d6:lengthi3e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let trackers = vec![
            "udp://one.example:1337/announce".to_string(),
            "http://two.example/announce".to_string(),
        ];
        let with = torrent_file(info, &trackers);
        let with = librqbit::torrent_from_bytes(&with).unwrap();
        let announced: Vec<String> = with
            .iter_announce()
            .map(|t| String::from_utf8_lossy(t.as_ref()).into_owned())
            .collect();
        assert_eq!(announced, trackers);
        let bare = torrent_file(info, &[]);
        let bare = librqbit::torrent_from_bytes(&bare).unwrap();
        assert_eq!(bare.info_hash, with.info_hash);
        assert_eq!(bare.iter_announce().count(), 0);
    }

    #[test]
    fn trackers_are_checked_and_deduplicated() {
        let clean = clean_trackers(vec![
            " udp://one.example:1337/announce ".into(),
            String::new(),
            "udp://one.example:1337/announce".into(),
            "https://two.example/announce".into(),
        ])
        .unwrap();
        assert_eq!(
            clean,
            [
                "udp://one.example:1337/announce",
                "https://two.example/announce"
            ]
        );
        assert!(clean_trackers(vec!["not a tracker".into()]).is_err());
        assert!(clean_trackers(vec!["ftp://three.example/".into()]).is_err());
    }

    #[tokio::test]
    async fn a_limit_below_one_request_still_lets_requests_through() {
        let limits = librqbit::limits::Limits::new(LimitsConfig {
            download_bps: bps(5000),
            upload_bps: bps(5000),
        });
        let request = NonZeroU32::new(16 * 1024).unwrap();
        let wait = Duration::from_secs(1);
        let up = tokio::time::timeout(wait, limits.prepare_for_upload(request)).await;
        assert!(matches!(up, Ok(Ok(()))));
        let down = tokio::time::timeout(wait, limits.prepare_for_download(request)).await;
        assert!(matches!(down, Ok(Ok(()))));
    }

    async fn wait_for(core: &Core, what: &str, check: impl Fn(&Torrent) -> bool) {
        for _ in 0..300 {
            if core.snapshot().torrents.first().is_some_and(&check) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("no {what}: {:?}", core.snapshot().torrents.first());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_seeding_torrent_changes_trackers_rechecks_and_keeps_its_choices_after_a_restart() {
        let root = tempfile::tempdir().unwrap();
        let downloads = root.path().join("downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let data: Vec<u8> = (0..3 * 65536u32).map(|n| (n * 7 % 251) as u8).collect();
        std::fs::write(downloads.join("data.bin"), &data).unwrap();
        let options = librqbit::CreateTorrentOptions {
            name: None,
            trackers: Vec::new(),
            piece_length: Some(65536),
        };
        let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
        let created = librqbit::create_torrent(&downloads.join("data.bin"), options, &spawner)
            .await
            .unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let settings = Settings {
            download_dir: downloads.to_string_lossy().into_owned(),
            dht: false,
            upnp: false,
            ..Settings::default()
        };
        settings.save(&home.join("settings.json")).unwrap();

        let core = Core::start(home.clone()).await.unwrap();
        let bytes = created.as_bytes().unwrap().to_vec();
        let preview = core.preview(Source::File(bytes)).await.unwrap();
        let mut request = AddRequest::link("");
        request.source = Source::Resolved(preview.id.clone());
        request.save_path = Some(settings.download_dir.clone());
        let id = core.add(request).await.unwrap();
        let seeding = |t: &Torrent| t.state == State::Seeding;
        wait_for(&core, "seeding after the first check", seeding).await;

        let trackers = vec![
            "udp://one.example:1337/announce".to_string(),
            "http://two.example/announce".to_string(),
        ];
        let mut sorted = trackers.clone();
        sorted.sort();
        let change = Action::SetTrackers {
            id: id.clone(),
            trackers: trackers.clone(),
        };
        core.execute(change).await.unwrap();
        let pieces = home.join("session").join(format!("{id}.bitv"));
        assert!(pieces.exists(), "the saved pieces should come back");
        wait_for(&core, "seeding after the tracker change", seeding).await;
        assert_eq!(core.details(&id).unwrap().trackers, sorted);

        let ids = vec![id.clone()];
        let quiet = Action::SetNoUpload {
            ids: ids.clone(),
            on: true,
        };
        core.execute(quiet).await.unwrap();
        let limits = Action::SetLimits {
            ids: ids.clone(),
            down: 5000,
            up: 7000,
        };
        core.execute(limits).await.unwrap();
        let handle = core.handle(&id).unwrap();
        assert!(handle.upload_disabled());
        assert_eq!(handle.ratelimits().get_upload_bps().unwrap().get(), 7000);

        core.execute(Action::Recheck { ids }).await.unwrap();
        assert!(core.handle(&id).unwrap().upload_disabled());
        wait_for(&core, "seeding after the recheck", seeding).await;

        core.shutdown().await;
        drop(core);
        let core = Core::start(home).await.unwrap();
        wait_for(&core, "seeding after a restart", seeding).await;
        assert_eq!(core.details(&id).unwrap().trackers, sorted);
        let handle = core.handle(&id).unwrap();
        assert!(handle.upload_disabled());
        assert_eq!(handle.ratelimits().get_download_bps().unwrap().get(), 5000);
        let row = core.snapshot().torrents[0].clone();
        assert!(row.no_upload);
        assert_eq!((row.down_limit, row.up_limit), (5000, 7000));
        core.shutdown().await;
    }

    async fn shared_file(root: &Path, name: &str) -> (PathBuf, Vec<u8>) {
        let folder = root.join(format!("{name}-data"));
        std::fs::create_dir_all(&folder).unwrap();
        let seed = name.len() as u32;
        let data: Vec<u8> = (0..3 * 65536u32)
            .map(|n| ((n + seed) * 7 % 251) as u8)
            .collect();
        let file = folder.join(format!("{name}.bin"));
        std::fs::write(&file, &data).unwrap();
        let options = librqbit::CreateTorrentOptions {
            name: None,
            trackers: Vec::new(),
            piece_length: Some(65536),
        };
        let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
        let created = librqbit::create_torrent(&file, options, &spawner)
            .await
            .unwrap();
        (folder, created.as_bytes().unwrap().to_vec())
    }

    async fn start_core(home: &Path, downloads: &Path, tweak: impl FnOnce(&mut Settings)) -> Core {
        std::fs::create_dir_all(home).unwrap();
        let mut settings = Settings {
            download_dir: downloads.to_string_lossy().into_owned(),
            dht: false,
            upnp: false,
            listen_port: 0,
            ..Settings::default()
        };
        tweak(&mut settings);
        settings.save(&home.join("settings.json")).unwrap();
        Core::start(home.to_path_buf()).await.unwrap()
    }

    async fn add_file(core: &Core, bytes: Vec<u8>, save: &Path, skip_check: bool) -> Id {
        let preview = core.preview(Source::File(bytes)).await.unwrap();
        let mut request = AddRequest::link("");
        request.source = Source::Resolved(preview.id);
        request.save_path = Some(save.to_string_lossy().into_owned());
        request.skip_check = skip_check;
        core.add(request).await.unwrap()
    }

    async fn wait_until(core: &Core, id: &str, what: &str, check: impl Fn(&Torrent) -> bool) {
        let row = || {
            core.snapshot()
                .torrents
                .iter()
                .find(|t| t.id == id)
                .cloned()
        };
        for _ in 0..300 {
            if row().is_some_and(|t| check(&t)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("no {what}: {:?}", row());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_finished_download_runs_the_finish_program_and_tells_subscribers() {
        let root = tempfile::tempdir().unwrap();
        let (seed_dir, bytes) = shared_file(root.path(), "data").await;
        let seeder = start_core(&root.path().join("seeder"), &seed_dir, |_| {}).await;
        let id = add_file(&seeder, bytes.clone(), &seed_dir, false).await;
        wait_until(&seeder, &id, "seeding", |t| t.state == State::Seeding).await;
        let port = seeder.snapshot().totals.port.expect("the seeder listens");

        let out = root.path().join("finished.txt");
        let command = if cfg!(windows) {
            format!("cmd /C echo %N %Z>{}", out.display())
        } else {
            format!("sh -c 'echo %N %Z > \"{}\"'", out.display())
        };
        let downloads = root.path().join("leech");
        let leecher = start_core(&root.path().join("leecher"), &downloads, |settings| {
            settings.on_finish = command;
        })
        .await;
        let mut finished = leecher.subscribe_finished();
        let preview = leecher.preview(Source::File(bytes)).await.unwrap();
        if let Some(resolved) = lock(&leecher.inner.resolved).get_mut(&preview.id) {
            resolved.peers = vec![SocketAddr::from(([127, 0, 0, 1], port))];
        }
        let mut request = AddRequest::link("");
        request.source = Source::Resolved(preview.id);
        leecher.add(request).await.unwrap();

        let wait = Duration::from_secs(60);
        let done = tokio::time::timeout(wait, finished.recv())
            .await
            .expect("the download finishes")
            .unwrap();
        assert_eq!(done.id, id);
        assert_eq!(done.name, "data.bin");
        assert!(downloads.join("data.bin").exists());
        let mut written = String::new();
        for _ in 0..100 {
            written = std::fs::read_to_string(&out).unwrap_or_default();
            if !written.trim().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(written.trim(), "data.bin 196608");
        leecher.shutdown().await;
        seeder.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn skipping_the_check_trusts_complete_files_and_still_catches_missing_ones() {
        let root = tempfile::tempdir().unwrap();
        let (complete_dir, complete) = shared_file(root.path(), "complete").await;
        let (missing_dir, missing) = shared_file(root.path(), "missing").await;
        std::fs::remove_dir_all(&missing_dir).unwrap();
        let core = start_core(&root.path().join("home"), root.path(), |_| {}).await;
        let mut finished = core.subscribe_finished();

        let trusted = add_file(&core, complete, &complete_dir, true).await;
        wait_until(&core, &trusted, "seeding", |t| t.state == State::Seeding).await;
        let wrong = add_file(&core, missing, &missing_dir, true).await;
        wait_until(&core, &wrong, "a download", |t| {
            matches!(t.state, State::Stalled | State::Downloading)
        })
        .await;
        let row = core
            .snapshot()
            .torrents
            .iter()
            .find(|t| t.id == wrong)
            .cloned();
        assert_eq!(row.unwrap().done, 0);
        assert!(finished.try_recv().is_err(), "nothing was downloaded");
        core.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_magnet_still_fetching_its_files_comes_back_after_a_restart() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let core = start_core(&home, root.path(), |_| {}).await;
        let link = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=waiting";
        let mut request = AddRequest::link(link);
        request.category = Some("tv".into());
        let id = core.add(request).await.unwrap();
        let waiting = |t: &Torrent| {
            t.name == "waiting"
                && t.category.as_deref() == Some("tv")
                && matches!(t.state, State::Metadata | State::Error)
        };
        wait_until(&core, &id, "a row for the magnet", waiting).await;
        core.shutdown().await;
        drop(core);

        let core = Core::start(home.clone()).await.unwrap();
        wait_until(&core, &id, "the row after a restart", waiting).await;
        let remove = Action::Remove {
            ids: vec![id.clone()],
            delete_files: false,
        };
        core.execute(remove).await.unwrap();
        core.shutdown().await;
        drop(core);

        let core = Core::start(home).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(core.snapshot().torrents.iter().all(|t| t.id != id));
        core.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn category_limits_follow_renames_and_backups_carry_categories_tags_and_rss() {
        let root = tempfile::tempdir().unwrap();
        let (folder, bytes) = shared_file(root.path(), "data").await;
        let core = start_core(&root.path().join("home"), &folder, |_| {}).await;
        let id = add_file(&core, bytes, &folder, false).await;
        let ids = vec![id.clone()];
        let run = |action: Action| {
            let core = core.clone();
            async move { core.execute(action).await.unwrap() }
        };
        run(Action::SetCategory {
            ids: ids.clone(),
            category: Some("iso".into()),
        })
        .await;
        run(Action::SetCategoryLimits {
            name: "iso".into(),
            down: 5000,
            up: 7000,
        })
        .await;
        let group = core.handle(&id).unwrap().group_ratelimits();
        assert_eq!(group.unwrap().get_upload_bps().unwrap().get(), 7000);

        run(Action::RenameCategory {
            from: "iso".into(),
            to: "linux".into(),
        })
        .await;
        run(Action::AddTags {
            ids: ids.clone(),
            tags: vec!["old".into(), "keep".into()],
        })
        .await;
        run(Action::RenameTag {
            from: "old".into(),
            to: "new".into(),
        })
        .await;
        let snapshot = core.snapshot();
        let linux = Category {
            name: "linux".into(),
            save_path: String::new(),
            down_limit: 5000,
            up_limit: 7000,
        };
        assert_eq!(snapshot.categories, std::slice::from_ref(&linux));
        assert_eq!(snapshot.torrents[0].category.as_deref(), Some("linux"));
        assert_eq!(snapshot.torrents[0].tags, ["keep", "new"]);
        let group = core.handle(&id).unwrap().group_ratelimits();
        assert_eq!(group.unwrap().get_download_bps().unwrap().get(), 5000);
        let renamed = core
            .execute(Action::RenameCategory {
                from: "linux".into(),
                to: "linux".into(),
            })
            .await;
        assert!(renamed.is_ok());

        let rule = Rule {
            name: "shows".into(),
            must_contain: "show 1080p".into(),
            category: Some("linux".into()),
            ..Rule::default()
        };
        core.rss_execute(RssAction::SaveRule { rule })
            .await
            .unwrap();
        core.rss_execute(RssAction::AddFeed {
            url: "http://127.0.0.1:9/feed".into(),
        })
        .await
        .unwrap();
        let backup = core.backup();
        assert!(backup.settings.web.password_hash.is_empty());

        let other = start_core(&root.path().join("other"), root.path(), |_| {}).await;
        let mut wrong = backup.clone();
        wrong.app = "something else".into();
        assert!(other.restore(wrong).await.is_err());
        other.restore(backup).await.unwrap();
        assert_eq!(other.snapshot().categories, [linux]);
        assert!(other.snapshot().tags.contains(&"new".to_string()));
        let rss = other.rss();
        assert_eq!(rss.rules[0].name, "shows");
        assert_eq!(rss.feeds[0].url, "http://127.0.0.1:9/feed");
        assert_eq!(other.settings().download_dir, core.settings().download_dir);

        run(Action::SetCategoryLimits {
            name: "linux".into(),
            down: 0,
            up: 0,
        })
        .await;
        assert!(core.handle(&id).unwrap().group_ratelimits().is_none());
        other.shutdown().await;
        core.shutdown().await;
    }

    #[test]
    fn finish_commands_split_like_a_shell_and_fill_placeholders() {
        assert_eq!(
            split_command(r#"C:\scripts\done.bat "%F" --tag '%G'  "#),
            [r"C:\scripts\done.bat", "%F", "--tag", "%G"]
        );
        assert_eq!(split_command(r#"say "a b"c"#), ["say", "a bc"]);
        assert_eq!(split_command(r#"x """#), ["x", ""]);
        assert!(split_command("   ").is_empty());
        let values = [('N', "My; Show".to_string()), ('Z', "42".to_string())];
        assert_eq!(
            fill_placeholders("%N (%Z) 100%% %Q %", &values),
            "My; Show (42) 100% %Q %"
        );
    }

    #[test]
    fn a_full_bitfield_sets_one_bit_per_piece_from_the_top() {
        assert_eq!(full_bitfield(8), [0xff]);
        assert_eq!(full_bitfield(10), [0xff, 0b1100_0000]);
        assert_eq!(full_bitfield(1), [0b1000_0000]);
    }

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
