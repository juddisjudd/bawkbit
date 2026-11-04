use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui_kit::*;

use crate::updater::{self, Release};

/// A first check soon after start, out of the way of the engine coming up.
const FIRST_CHECK: Duration = Duration::from_secs(10);
/// bawkbit can seed for weeks without a restart, so it looks again once a day.
const NEXT_CHECK: Duration = Duration::from_secs(24 * 60 * 60);
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    Current,
    Available(Release),
    Downloading {
        version: String,
        done: u64,
        total: u64,
    },
    Ready {
        version: String,
        path: PathBuf,
    },
    Failed(String),
}

pub struct UpdateFound(pub String);

/// Whether a newer bawkbit exists, and installing it; the network work runs on the engine's runtime.
pub struct Updates {
    pub state: UpdateState,
    automatic: bool,
    runtime: tokio::runtime::Handle,
    _schedule: Option<Task<()>>,
    _task: Option<Task<()>>,
}

impl EventEmitter<UpdateFound> for Updates {}

impl Updates {
    pub fn new(automatic: bool, runtime: tokio::runtime::Handle, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            state: UpdateState::Idle,
            automatic: false,
            runtime,
            _schedule: None,
            _task: None,
        };
        this.set_automatic(automatic, cx);
        this
    }

    pub fn automatic(&self) -> bool {
        self.automatic
    }

    /// True when there is something to do: a version to install, or one installed and waiting for a restart.
    pub fn has_news(&self) -> bool {
        matches!(
            self.state,
            UpdateState::Available(_) | UpdateState::Ready { .. }
        )
    }

    pub fn set_automatic(&mut self, on: bool, cx: &mut Context<Self>) {
        if on == self.automatic {
            return;
        }
        self.automatic = on;
        self._schedule = on.then(|| {
            cx.spawn(async move |this, cx| {
                let mut wait = FIRST_CHECK;
                loop {
                    cx.background_executor().timer(wait).await;
                    wait = NEXT_CHECK;
                    let checked = this.update(cx, |this, cx| {
                        if !this.has_news() {
                            this.check(cx);
                        }
                    });
                    if checked.is_err() {
                        break;
                    }
                }
            })
        });
        cx.notify();
    }

    pub fn check(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.state,
            UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Ready { .. }
        ) {
            return;
        }
        self.state = UpdateState::Checking;
        cx.notify();
        let job = self.runtime.spawn(updater::check());
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|err| Err(err.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(Some(release)) => {
                        log::info!("bawkbit {} is out", release.version);
                        cx.emit(UpdateFound(release.version.clone()));
                        UpdateState::Available(release)
                    }
                    Ok(None) => UpdateState::Current,
                    Err(error) => {
                        log::warn!("cannot check for updates: {error}");
                        UpdateState::Failed(error)
                    }
                };
                cx.notify();
            });
        }));
    }

    pub fn install(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(release) = self.state.clone() else {
            return;
        };
        let (version, total) = (release.version.clone(), release.size);
        self.state = UpdateState::Downloading {
            version: version.clone(),
            done: 0,
            total,
        };
        cx.notify();
        let done = Arc::new(AtomicU64::new(0));
        let worker_done = done.clone();
        let job = self.runtime.spawn(async move {
            updater::install(&release, move |bytes| {
                worker_done.store(bytes, Ordering::Relaxed)
            })
            .await
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            while !job.is_finished() {
                cx.background_executor().timer(PROGRESS_EVERY).await;
                let progress = UpdateState::Downloading {
                    version: version.clone(),
                    done: done.load(Ordering::Relaxed),
                    total,
                };
                let alive = this.update(cx, |this, cx| {
                    this.state = progress;
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let outcome = job.await.unwrap_or_else(|err| Err(err.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.state = match outcome {
                    Ok(path) => {
                        log::info!("installed bawkbit {version} at {}", path.display());
                        UpdateState::Ready { version, path }
                    }
                    Err(error) => {
                        log::warn!("cannot install the update: {error}");
                        UpdateState::Failed(error)
                    }
                };
                cx.notify();
            });
        }));
    }

    /// Starts the new version and quits this one, which saves the torrents on the way out.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Ready { path, .. } = &self.state else {
            return;
        };
        match updater::relaunch(path) {
            Ok(()) => cx.quit(),
            Err(error) => {
                self.state = UpdateState::Failed(error);
                cx.notify();
            }
        }
    }
}
