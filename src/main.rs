#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod config;
mod engine;
mod format;
mod instance;
mod logging;
mod theme;
mod ui;

use bawkbit_core::Core;
use gpui_kit::component::TitleBar;
use gpui_kit::*;

use engine::Engine;
use instance::Claim;

fn main() {
    logging::init();
    let args = instance::launch_args();
    let home = bawkbit_core::data_dir();
    let listener = match instance::claim(&home, &args) {
        Claim::Forwarded => return,
        Claim::Primary(listener) => listener,
    };

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

    application().with_assets(assets::AppAssets).run(move |cx| {
        #[cfg(target_os = "linux")]
        theme::add_system_font(cx);
        init(cx);
        cx.set_app_identity("com.bawkbit.app", "bawkbit");
        theme::init(cx);
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
        let handle = runtime.handle().clone();
        let opened = match started {
            Ok((core, web)) => {
                let (sender, inbox) = async_channel::unbounded();
                if let Some(listener) = listener {
                    instance::listen(listener, sender);
                }
                open_window(options, cx, move |window, cx| {
                    window.set_window_title("bawkbit");
                    let engine = cx.new(|cx| Engine::new(core, handle, web, cx));
                    cx.new(|cx| ui::Workspace::new(engine, args, inbox, window, cx))
                })
                .map(|_| ())
            }
            Err(err) => {
                let message = format!("{err:#}");
                log::error!("{message}");
                open_window(options, cx, move |window, cx| {
                    window.set_window_title("bawkbit");
                    cx.new(|_| ui::Failed::new(message))
                })
                .map(|_| ())
            }
        };
        opened.expect("failed to open window");
        cx.activate(true);
        cx.set_global(Runtime(Some(runtime)));
    });
}

/// Keeps the engine's threads alive for as long as the app runs.
struct Runtime(Option<tokio::runtime::Runtime>);

impl Global for Runtime {}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_timeout(std::time::Duration::from_secs(2));
        }
    }
}
