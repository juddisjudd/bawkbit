#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod autostart;
mod config;
mod engine;
mod format;
mod instance;
mod logging;
mod shell;
mod theme;
mod tray;
mod ui;
mod updater;

use std::time::Duration;

use bawkbit_core::Core;
use gpui_kit::component::TitleBar;
use gpui_kit::*;

use config::Config;
use engine::Engine;
use instance::Claim;

fn main() {
    logging::init();
    let args = instance::launch_args();
    let home = bawkbit_core::data_dir();
    let after_update = std::env::args().any(|arg| arg == updater::AFTER_UPDATE);
    let hidden = std::env::args().any(|arg| arg == autostart::HIDDEN);
    let patience = Duration::from_secs(if after_update { 30 } else { 0 });
    let listener = match instance::claim(&home, &args, patience) {
        Claim::Forwarded => return,
        Claim::Primary(listener) => listener,
    };
    #[cfg(windows)]
    {
        updater::clean_up();
        updater::sync_installed_version();
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("bawkbit-engine")
        .build()
        .expect("can start the engine's threads");
    let started = runtime.block_on(Core::start(home)).map(|core| {
        let web = {
            let _entered = runtime.enter();
            bawkbit_web::spawn(core.clone())
        };
        (core, web)
    });

    application()
        .with_assets(assets::AppAssets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            #[cfg(target_os = "linux")]
            theme::add_system_font(cx);
            init(cx);
            cx.set_app_identity("com.bawkbit.app", "bawkbit");
            theme::init(cx);
            cx.set_global(Config::load());
            let handle = runtime.handle().clone();
            match started {
                Ok((core, web)) => {
                    let (sender, inbox) = async_channel::unbounded();
                    if let Some(listener) = listener {
                        instance::listen(listener, sender);
                    }
                    let engine = cx.new(|cx| Engine::new(core, handle, web, cx));
                    shell::start(engine, args, inbox, hidden, cx);
                }
                Err(err) => {
                    let message = format!("{err:#}");
                    log::error!("{message}");
                    show_failure(message, cx);
                }
            }
            cx.set_global(Runtime(Some(runtime)));
        });
}

/// A window that says why the engine did not start; closing it quits.
fn show_failure(message: String, cx: &mut App) {
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(820.), px(520.))),
        app_id: Some("bawkbit".into()),
        ..TitleBar::window_options()
    };
    let opened = open_window(options, cx, move |window, cx| {
        window.set_window_title("bawkbit");
        cx.new(|_| ui::Failed::new(message))
    });
    if let Err(err) = opened {
        log::error!("cannot open the window: {err:#}");
        cx.quit();
    }
    cx.activate(true);
}

/// Keeps the engine's threads alive for as long as the app runs.
struct Runtime(Option<tokio::runtime::Runtime>);

impl Global for Runtime {}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_timeout(Duration::from_secs(2));
        }
    }
}
