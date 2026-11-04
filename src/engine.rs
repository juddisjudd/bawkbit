use std::future::Future;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use bawkbit_core::{Core, Snapshot};
use bawkbit_web::{Status, WebUi};
use gpui_kit::*;

/// The engine as the window sees it: the latest snapshot, and a way to run engine work on the engine's runtime.
pub struct Engine {
    core: Core,
    runtime: tokio::runtime::Handle,
    web: WebUi,
    pub snapshot: Arc<Snapshot>,
    _pump: Task<()>,
}

impl Engine {
    pub fn new(
        core: Core,
        runtime: tokio::runtime::Handle,
        web: WebUi,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut updates = core.subscribe();
        let snapshot = updates.borrow_and_update().clone();
        let pump = cx.spawn(async move |this, cx| {
            while updates.changed().await.is_ok() {
                let snapshot = updates.borrow_and_update().clone();
                let alive = this.update(cx, |this, cx| {
                    this.snapshot = snapshot;
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Self {
            core,
            runtime,
            web,
            snapshot,
            _pump: pump,
        }
    }

    pub fn core(&self) -> &Core {
        &self.core
    }

    pub fn runtime(&self) -> tokio::runtime::Handle {
        self.runtime.clone()
    }

    pub fn web_status(&self) -> Status {
        self.web.status()
    }

    /// Runs `job` on the engine's runtime; the returned future can be awaited from the window's executor.
    pub fn run<T, F, J>(&self, job: J) -> impl Future<Output = Result<T>> + use<T, F, J>
    where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
        J: FnOnce(Core) -> F,
    {
        let handle = self.runtime.spawn(job(self.core.clone()));
        async move { handle.await.map_err(|err| anyhow!("{err}"))? }
    }

    pub fn shutdown(&self) -> impl Future<Output = ()> + use<> {
        let core = self.core.clone();
        let handle = self.runtime.spawn(async move { core.shutdown().await });
        async move {
            let _ = handle.await;
        }
    }
}
