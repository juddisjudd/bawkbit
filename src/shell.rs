use bawkbit_core::{Action, Finished};
use gpui_kit::component::TitleBar;
use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;
use tokio::sync::broadcast::error::RecvError;

use crate::config::Config;
use crate::engine::Engine;
use crate::format;
use crate::tray::{Tray, TrayCommand};
use crate::ui::Workspace;
use crate::ui::updates::{UpdateFound, Updates};

const FINISHED_TAG: &str = "finished:";
const UPDATE_TAG: &str = "update";

struct Main {
    handle: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
}

/// The part of bawkbit that outlives its window: the engine, update checks, the tray icon and notifications.
pub struct Shell {
    engine: Entity<Engine>,
    updates: Entity<Updates>,
    window: Option<Main>,
    tray: Option<Tray>,
    commands: async_channel::Sender<TrayCommand>,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

/// Holds the shell for as long as the app runs; its tasks and subscriptions only hold it weakly.
struct ShellHandle {
    _shell: Entity<Shell>,
}

impl Global for ShellHandle {}

/// Opens the window, unless `hidden` and a tray icon can stand in for it.
pub fn start(
    engine: Entity<Engine>,
    args: Vec<String>,
    inbox: async_channel::Receiver<Vec<String>>,
    hidden: bool,
    cx: &mut App,
) {
    let shell = cx.new(|cx| Shell::new(engine, inbox, cx));
    cx.set_global(ShellHandle {
        _shell: shell.clone(),
    });
    let weak = shell.downgrade();
    cx.on_system_notification_response(move |response, cx| {
        let tag = response.tag.to_string();
        let _ = weak.update(cx, |shell, cx| {
            shell.show(Vec::new(), cx);
            if let Some(id) = tag.strip_prefix(FINISHED_TAG) {
                let id = id.to_string();
                shell.with_workspace(cx, move |workspace, window, cx| {
                    workspace.reveal(id, window, cx)
                });
            } else if tag == UPDATE_TAG {
                shell.with_workspace(cx, |workspace, window, cx| workspace.show_about(window, cx));
            }
        });
    });
    let in_tray = hidden && shell.read(cx).tray.is_some();
    if !in_tray {
        shell.update(cx, |shell, cx| shell.show(args, cx));
    }
}

impl Shell {
    fn new(
        engine: Entity<Engine>,
        inbox: async_channel::Receiver<Vec<String>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = cx.global::<Config>().clone();
        let runtime = engine.read(cx).runtime();
        let updates = cx.new(|cx| Updates::new(config.check_updates, runtime, cx));
        let (commands, tray_inbox) = async_channel::unbounded();

        let weak = cx.entity().downgrade();
        let subscriptions = vec![
            cx.observe(&engine, |this, _, cx| this.refresh_tray(cx)),
            cx.observe_global::<Config>(|this, cx| this.sync_tray(cx)),
            cx.subscribe(&updates, |this, _, found: &UpdateFound, cx| {
                if this.window.is_none() {
                    cx.show_system_notification(SystemNotification {
                        tag: UPDATE_TAG.into(),
                        title: format!("bawkbit {} is out", found.0).into(),
                        body: "open bawkbit to update.".into(),
                        actions: Vec::new(),
                    });
                }
            }),
            cx.on_window_closed(move |cx, closed| {
                let _ = weak.update(cx, |this, cx| this.closed(closed, cx));
            }),
            cx.on_app_quit(|this, cx| this.engine.read(cx).shutdown()),
        ];

        let mut finished = engine.read(cx).core().subscribe_finished();
        let tasks = vec![
            cx.spawn(async move |this, cx| {
                while let Ok(args) = inbox.recv().await {
                    if this.update(cx, |this, cx| this.show(args, cx)).is_err() {
                        break;
                    }
                }
            }),
            cx.spawn(async move |this, cx| {
                while let Ok(command) = tray_inbox.recv().await {
                    if this
                        .update(cx, |this, cx| this.on_tray(command, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            }),
            cx.spawn(async move |this, cx| {
                loop {
                    let done = match finished.recv().await {
                        Ok(done) => done,
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    };
                    if this
                        .update(cx, |this, cx| this.on_finished(done, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            }),
        ];

        let mut this = Self {
            engine,
            updates,
            window: None,
            tray: None,
            commands,
            _subscriptions: subscriptions,
            _tasks: tasks,
        };
        this.sync_tray(cx);
        this
    }

    /// Brings the window to the front, opening it when it is closed, and hands it any links.
    fn show(&mut self, args: Vec<String>, cx: &mut Context<Self>) {
        if let Some(main) = &self.window {
            let workspace = main.workspace.clone();
            let links = args.clone();
            let shown = main.handle.update(cx, |_, window, cx| {
                window.activate_window();
                if !links.is_empty() {
                    let _ = workspace
                        .update(cx, |workspace, cx| workspace.open_links(links, window, cx));
                }
            });
            if shown.is_ok() {
                return;
            }
        }
        let (engine, updates) = (self.engine.clone(), self.updates.clone());
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(820.), px(520.))),
            app_id: Some("bawkbit".into()),
            ..TitleBar::window_options()
        };
        let opened = open_window(options, cx, move |window, cx| {
            window.set_window_title("bawkbit");
            cx.new(|cx| Workspace::new(engine, updates, args, window, cx))
        });
        match opened {
            Ok((handle, workspace)) => {
                self.window = Some(Main {
                    handle,
                    workspace: workspace.downgrade(),
                });
                cx.activate(true);
            }
            Err(err) => {
                log::error!("cannot open the window: {err:#}");
                cx.quit();
            }
        }
    }

    fn with_workspace(
        &self,
        cx: &mut Context<Self>,
        run: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    ) {
        let Some(main) = &self.window else {
            return;
        };
        let workspace = main.workspace.clone();
        let _ = main.handle.update(cx, |_, window, cx| {
            let _ = workspace.update(cx, |workspace, cx| run(workspace, window, cx));
        });
    }

    /// Keeps running when the tray can bring the window back, and quits otherwise.
    fn closed(&mut self, closed: WindowId, cx: &mut Context<Self>) {
        if self
            .window
            .as_ref()
            .is_none_or(|main| main.handle.window_id() != closed)
        {
            return;
        }
        self.window = None;
        let config = cx.global::<Config>();
        if self.tray.is_none() || !config.close_to_tray {
            cx.quit();
            return;
        }
        if !config.told_about_tray {
            cx.show_system_notification(SystemNotification {
                tag: "tray".into(),
                title: "bawkbit is still running".into(),
                body: "it keeps downloading and seeding. quit from the tray icon.".into(),
                actions: Vec::new(),
            });
            Config::edit(cx, |config| config.told_about_tray = true);
        }
    }

    fn sync_tray(&mut self, cx: &mut Context<Self>) {
        let wanted = cx.global::<Config>().tray;
        if wanted && self.tray.is_none() {
            let runtime = self.engine.read(cx).runtime();
            self.tray = Tray::new(self.commands.clone(), &runtime);
            self.refresh_tray(cx);
        } else if !wanted {
            self.tray = None;
        }
    }

    fn refresh_tray(&mut self, cx: &mut Context<Self>) {
        let Some(tray) = &mut self.tray else {
            return;
        };
        let totals = &self.engine.read(cx).snapshot.totals;
        let tooltip = format!(
            "bawkbit\n↓ {}  ↑ {}",
            format::speed(totals.down),
            format::speed(totals.up)
        );
        tray.update(tooltip, totals.alt_speed);
    }

    fn on_tray(&mut self, command: TrayCommand, cx: &mut Context<Self>) {
        let action = match command {
            TrayCommand::Open => return self.show(Vec::new(), cx),
            TrayCommand::Quit => return cx.quit(),
            TrayCommand::PauseAll => Action::PauseAll,
            TrayCommand::ResumeAll => Action::ResumeAll,
            TrayCommand::ToggleAltSpeed => Action::SetAltSpeed {
                on: !self.engine.read(cx).snapshot.totals.alt_speed,
            },
        };
        let job = self
            .engine
            .read(cx)
            .run(move |core| async move { core.execute(action).await.map(|_| ()) });
        cx.background_spawn(async move {
            if let Err(err) = job.await {
                log::warn!("{err:#}");
            }
        })
        .detach();
    }

    /// An in-app note while the window has focus, and a system notification otherwise.
    fn on_finished(&mut self, done: Finished, cx: &mut Context<Self>) {
        if !cx.global::<Config>().notify_finished {
            return;
        }
        let note = format!("{} finished", done.name);
        if let Some(main) = &self.window {
            let shown = main.handle.update(cx, |_, window, cx| {
                let active = window.is_window_active();
                if active {
                    window.push_notification(Notification::success(note.clone()), cx);
                }
                active
            });
            if matches!(shown, Ok(true)) {
                return;
            }
        }
        cx.show_system_notification(SystemNotification {
            tag: format!("{FINISHED_TAG}{}", done.id).into(),
            title: done.name.into(),
            body: "finished downloading".into(),
            actions: Vec::new(),
        });
    }
}
