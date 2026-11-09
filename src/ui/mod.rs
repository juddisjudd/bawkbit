mod add;
mod chrome;
mod details;
mod filter;
mod kit;
mod list;
mod prompts;
mod rss;
mod search;
mod settings;
pub mod updates;

use bawkbit_core::{Action, AddRequest, Source};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{WindowExt, resizable_panel, v_resizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::config::Config;
use crate::engine::Engine;
use crate::theme::{self, Mode, palette};

use add::{AddDone, AddView};
use details::{DetailsEvent, DetailsView};
use filter::Status;
use list::{ListEvent, TorrentList};
use rss::{RssEvent, RssView};
use search::{SearchEvent, SearchView};
use settings::{SettingsEvent, SettingsView};
use updates::{UpdateFound, Updates};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Torrents,
    Search,
    Rss,
    Settings,
}

/// Runs an engine action and shows its error, if any, as a notification.
pub fn act<V: 'static>(
    engine: &Entity<Engine>,
    action: Action,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let job = engine
        .read(cx)
        .run(move |core| async move { core.execute(action).await.map(|_| ()) });
    cx.spawn_in(window, async move |_, cx| {
        if let Err(err) = job.await {
            let _ = cx.update(|window, cx| {
                window.push_notification(Notification::error(format!("{err:#}")), cx)
            });
        }
    })
    .detach();
}

pub struct Workspace {
    engine: Entity<Engine>,
    page: Page,
    list: Entity<TorrentList>,
    details: Entity<DetailsView>,
    search: Entity<SearchView>,
    rss: Entity<RssView>,
    settings: Entity<SettingsView>,
    updates: Entity<Updates>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        engine: Entity<Engine>,
        updates: Entity<Updates>,
        args: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = cx.global::<Config>().clone();
        theme::apply(&config.theme, config.mode, Some(window), cx);

        let list = cx.new(|cx| TorrentList::new(engine.clone(), &config, window, cx));
        let details = cx.new(|cx| DetailsView::new(engine.clone(), cx));
        let search = cx.new(|cx| SearchView::new(engine.clone(), window, cx));
        let rss = cx.new(|cx| RssView::new(engine.clone(), window, cx));
        let settings =
            cx.new(|cx| SettingsView::new(engine.clone(), updates.clone(), &config, window, cx));

        let subscriptions = vec![
            cx.observe(&engine, |_, _, cx| cx.notify()),
            cx.subscribe_in(&list, window, Self::on_list),
            cx.subscribe_in(&settings, window, Self::on_settings),
            cx.subscribe_in(&search, window, Self::on_search),
            cx.subscribe_in(
                &rss,
                window,
                |this, _, event: &RssEvent, window, cx| match event {
                    RssEvent::Add(link) => this.add_source(Source::Link(link.clone()), window, cx),
                },
            ),
            cx.subscribe_in(&updates, window, |_, _, found: &UpdateFound, window, cx| {
                let note = format!(
                    "bawkbit {} is out. open settings → about to update.",
                    found.0
                );
                window.push_notification(note, cx);
            }),
            cx.observe(&updates, |_, _, cx| cx.notify()),
            cx.subscribe(&details, |this, _, _: &DetailsEvent, cx| {
                this.list.update(cx, |list, cx| list.clear_focus(cx));
            }),
            cx.observe_window_appearance(window, |_, window, cx| {
                let config = cx.global::<Config>();
                if config.mode == Mode::System {
                    let theme = config.theme.clone();
                    theme::apply(&theme, Mode::System, Some(window), cx);
                }
            }),
        ];

        let this = Self {
            engine,
            page: Page::Torrents,
            list,
            details,
            search,
            rss,
            settings,
            updates,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        window.focus(&this.focus, cx);
        if !args.is_empty() {
            cx.defer_in(window, move |this, window, cx| {
                this.open_links(args, window, cx)
            });
        }
        this
    }

    fn on_list(
        &mut self,
        _: &Entity<TorrentList>,
        event: &ListEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ListEvent::Focus(id) => {
                let id = id.clone();
                self.details.update(cx, |details, cx| details.show(id, cx));
            }
            ListEvent::Sorted(column, descending) => {
                let (column, descending) = (*column, *descending);
                Config::edit(cx, |config| {
                    config.sort = column;
                    config.descending = descending;
                });
            }
            ListEvent::Add => self.open_add(None, window, cx),
        }
        cx.notify();
    }

    fn on_search(
        &mut self,
        _: &Entity<SearchView>,
        event: &SearchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SearchEvent::Add(link) => self.add_source(Source::Link(link.clone()), window, cx),
            SearchEvent::Reveal(id) => {
                let id = id.clone();
                self.list.update(cx, |list, cx| list.reveal(id, cx));
                self.select(Page::Torrents, window, cx);
            }
            SearchEvent::Configure => {
                self.settings
                    .update(cx, |settings, cx| settings.show_search(cx));
                self.select(Page::Settings, window, cx);
            }
        }
    }

    fn on_settings(
        &mut self,
        _: &Entity<SettingsView>,
        event: &SettingsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SettingsEvent::Theme(id, mode) => {
                let (theme, mode) = (id.to_string(), *mode);
                theme::apply(&theme, mode, Some(window), cx);
                Config::edit(cx, |config| {
                    config.theme = theme;
                    config.mode = mode;
                });
            }
            SettingsEvent::QuickAdd(on) => {
                let on = *on;
                Config::edit(cx, |config| config.quick_add = on);
            }
            SettingsEvent::CheckUpdates(on) => {
                let on = *on;
                Config::edit(cx, |config| config.check_updates = on);
            }
        }
        cx.notify();
    }

    /// Opens the add dialog for each link or file, or adds magnets straight away when quick add is on.
    pub fn open_links(&mut self, links: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        for link in links {
            let source = if link.starts_with("magnet:") || link.contains("://") {
                Source::Link(link)
            } else {
                match std::fs::read(&link) {
                    Ok(bytes) => Source::File(bytes),
                    Err(err) => {
                        window.push_notification(
                            Notification::error(format!("cannot read {link}: {err}")),
                            cx,
                        );
                        continue;
                    }
                }
            };
            self.add_source(source, window, cx);
        }
    }

    fn add_source(&mut self, source: Source, window: &mut Window, cx: &mut Context<Self>) {
        let magnet = matches!(&source, Source::Link(link) if link.starts_with("magnet:"));
        if cx.global::<Config>().quick_add && magnet {
            let request = AddRequest {
                source,
                ..AddRequest::link(String::new())
            };
            let job = self
                .engine
                .read(cx)
                .run(move |core| async move { core.add(request).await });
            cx.spawn_in(window, async move |_, cx| {
                if let Err(err) = job.await {
                    let _ = cx.update(|window, cx| {
                        window.push_notification(Notification::error(format!("{err:#}")), cx)
                    });
                }
            })
            .detach();
            return;
        }
        self.open_add(Some(source), window, cx);
    }

    fn open_add(&mut self, source: Option<Source>, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Torrents;
        let engine = self.engine.clone();
        let view = cx.new(|cx| AddView::new(engine, source, window, cx));
        let list = self.list.clone();
        cx.subscribe_in(&view, window, move |_, _, done: &AddDone, window, cx| {
            window.close_dialog(cx);
            if let Some(id) = done.0.clone() {
                list.update(cx, |list, cx| list.reveal(id, cx));
            }
        })
        .detach();
        let focus = view.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.w(px(680.)).child(view.clone())
        });
        window.focus(&focus, cx);
    }

    fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("add".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let links = paths
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            let _ = this.update_in(cx, |this, window, cx| this.open_links(links, window, cx));
        })
        .detach();
    }

    fn select(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        match page {
            Page::Torrents => window.focus(&self.focus, cx),
            Page::Search => {
                let focus = self.search.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            Page::Rss => {
                let focus = self.rss.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            Page::Settings => {}
        }
        cx.notify();
    }

    pub fn reveal(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| list.reveal(id, cx));
        self.select(Page::Torrents, window, cx);
    }

    pub fn show_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings
            .update(cx, |settings, cx| settings.show_about(cx));
        self.select(Page::Settings, window, cx);
    }

    pub fn set_status(&mut self, status: Status, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| list.set_status(status, cx));
        self.select(Page::Torrents, window, cx);
    }

    pub fn toggle_category(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.list
            .update(cx, |list, cx| list.toggle_category(name, cx));
        self.select(Page::Torrents, window, cx);
    }

    pub fn toggle_tag(&mut self, tag: String, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| list.toggle_tag(tag, cx));
        self.select(Page::Torrents, window, cx);
    }

    /// Shortcuts that work whenever no text box has the keyboard.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let command = modifiers.control || modifiers.platform;
        let typing = window.focused(cx).is_some() && !self.focus.is_focused(window);
        match keystroke.key.as_str() {
            "o" if command => self.pick_files(window, cx),
            "v" if command && !typing => {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap_or_default();
                let links: Vec<String> = text
                    .split_whitespace()
                    .filter(|word| word.starts_with("magnet:") || word.starts_with("http"))
                    .map(str::to_string)
                    .collect();
                if links.is_empty() {
                    self.open_add(None, window, cx);
                } else {
                    self.open_links(links, window, cx);
                }
            }
            _ if typing || self.page != Page::Torrents => return,
            _ => {
                let handled = self
                    .list
                    .update(cx, |list, cx| list.on_key(keystroke, window, cx));
                if !handled {
                    return;
                }
            }
        }
        cx.stop_propagation();
    }

    fn on_drop(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let links = paths
            .paths()
            .iter()
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("torrent"))
            })
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if links.is_empty() {
            window.push_notification(Notification::warning("drop .torrent files here"), cx);
        } else {
            self.open_links(links, window, cx);
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let snapshot = self.engine.read(cx).snapshot.clone();
        let focused = self.details.read(cx).is_showing();

        let content = match self.page {
            Page::Settings => self.settings.clone().into_any_element(),
            Page::Search => self.search.clone().into_any_element(),
            Page::Rss => self.rss.clone().into_any_element(),
            Page::Torrents => v_resizable("split")
                .child(
                    resizable_panel()
                        .size_range(px(160.)..Pixels::MAX)
                        .child(self.list.clone()),
                )
                .child(
                    resizable_panel()
                        .visible(focused)
                        .size(px(300.))
                        .size_range(px(160.)..px(900.))
                        .child(self.details.clone()),
                )
                .into_any_element(),
        };
        let drop_bg = p.yolk_dim;

        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .bg(p.bg)
            .text_color(p.text)
            .text_size(px(13.))
            .child(chrome::title_bar(&snapshot, &self.engine, window, cx))
            .child(
                div()
                    .id("body")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .on_drop(cx.listener(Self::on_drop))
                    .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(drop_bg))
                    .child(self.render_sidebar(&snapshot, cx))
                    .child(div().flex_1().min_w_0().h_full().child(content)),
            )
    }
}

/// Shown instead of the app when the engine cannot start.
pub struct Failed {
    message: SharedString,
}

impl Failed {
    pub fn new(message: String) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Render for Failed {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .text_color(p.text)
            .text_size(px(13.))
            .child(chrome::bare_title_bar(cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .px_8()
                    .child(kit::dot(p.danger))
                    .child(
                        div()
                            .text_color(p.text_strong)
                            .child("bawkbit could not start"),
                    )
                    .child(
                        div()
                            .max_w(px(560.))
                            .text_center()
                            .text_color(p.text_weak)
                            .child(self.message.clone()),
                    )
                    .when(cfg!(windows), |this| {
                        this.child(
                            div().text_size(px(12.)).text_color(p.text_weaker).child(
                                "is another bawkbit, or another app on the same port, running?",
                            ),
                        )
                    }),
            )
    }
}
