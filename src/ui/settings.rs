use std::collections::HashMap;

use bawkbit_core::settings::{Indexer, IndexerKind};
use bawkbit_core::{Backup, Settings};
use bawkbit_web::Status as WebStatus;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::kit;
use super::updates::{UpdateState, Updates};
use crate::autostart;
use crate::config::Config;
use crate::engine::Engine;
use crate::format;
use crate::theme::{self, DEFAULT_THEME, Look, Mode, Palette, palette};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Downloads,
    Connection,
    Speed,
    Queue,
    Search,
    Web,
    App,
    Appearance,
    About,
}

impl Tab {
    const ALL: [Tab; 9] = [
        Tab::Downloads,
        Tab::Connection,
        Tab::Speed,
        Tab::Queue,
        Tab::Search,
        Tab::Web,
        Tab::App,
        Tab::Appearance,
        Tab::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::Downloads => "downloads",
            Tab::Connection => "connection",
            Tab::Speed => "speed",
            Tab::Queue => "queue",
            Tab::Search => "search",
            Tab::Web => "web ui",
            Tab::App => "app",
            Tab::Appearance => "appearance",
            Tab::About => "about",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Tab::Downloads => IconName::FolderDown,
            Tab::Connection => IconName::Network,
            Tab::Speed => IconName::Gauge,
            Tab::Queue => IconName::Hourglass,
            Tab::Search => IconName::Search,
            Tab::Web => IconName::Globe,
            Tab::App => IconName::AppWindow,
            Tab::Appearance => IconName::Palette,
            Tab::About => IconName::Info,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Field {
    DownloadDir,
    WatchDir,
    Port,
    PeerLimit,
    DownLimit,
    UpLimit,
    AltDown,
    AltUp,
    From,
    To,
    MaxActive,
    Ratio,
    SeedMinutes,
    WebAddress,
    WebPort,
    WebUser,
    OnFinish,
    RssMinutes,
}

pub enum SettingsEvent {
    Theme(SharedString, Mode),
    QuickAdd(bool),
    CheckUpdates(bool),
}

pub struct SettingsView {
    engine: Entity<Engine>,
    updates: Entity<Updates>,
    tab: Tab,
    theme: SharedString,
    mode: Mode,
    quick_add: bool,
    start_at_login: bool,
    inputs: HashMap<Field, Entity<InputState>>,
    errors: HashMap<Field, &'static str>,
    password: Entity<InputState>,
    indexers: Vec<IndexerRow>,
    _subscriptions: Vec<Subscription>,
}

/// One Prowlarr or Jackett in the search tab, with its own text boxes.
struct IndexerRow {
    id: String,
    kind: IndexerKind,
    enabled: bool,
    name: Entity<InputState>,
    url: Entity<InputState>,
    key: Entity<InputState>,
    test: Option<Result<String, String>>,
    testing: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    pub fn new(
        engine: Entity<Engine>,
        updates: Entity<Updates>,
        config: &Config,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let s = engine.read(cx).core().settings();
        let mut inputs = HashMap::new();
        let mut subscriptions = Vec::new();
        for (field, value) in field_values(&s) {
            let placeholder = placeholder(field);
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            });
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.commit(field, window, cx);
                    }
                },
            ));
            inputs.insert(field, input);
        }
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("new password")
                .masked(true)
        });
        subscriptions.push(cx.observe(&updates, |_, _, cx| cx.notify()));
        let stored = engine.read(cx).core().settings().indexers;
        let mut this = Self {
            engine,
            updates,
            tab: Tab::Downloads,
            theme: config.theme.clone().into(),
            mode: config.mode,
            quick_add: config.quick_add,
            start_at_login: autostart::enabled(),
            inputs,
            errors: HashMap::new(),
            password,
            indexers: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.indexers = stored
            .iter()
            .map(|indexer| this.indexer_row(indexer, window, cx))
            .collect();
        this
    }

    pub fn show_search(&mut self, cx: &mut Context<Self>) {
        self.tab = Tab::Search;
        cx.notify();
    }

    pub fn show_about(&mut self, cx: &mut Context<Self>) {
        self.tab = Tab::About;
        cx.notify();
    }

    fn indexer_row(
        &self,
        indexer: &Indexer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> IndexerRow {
        let key_hint = if indexer.api_key.is_empty() {
            "api key"
        } else {
            "api key saved; type a new one to replace it"
        };
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(indexer.kind.label())
                .default_value(indexer.name.clone())
        });
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(indexer.kind.default_url())
                .default_value(indexer.url.clone())
        });
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(key_hint)
                .masked(true)
        });
        let subscriptions = [&name, &url, &key]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.save_indexers(window, cx);
                    }
                })
            })
            .collect();
        IndexerRow {
            id: indexer.id.clone(),
            kind: indexer.kind,
            enabled: indexer.enabled,
            name,
            url,
            key,
            test: None,
            testing: false,
            _subscriptions: subscriptions,
        }
    }

    fn indexer_from_row(row: &IndexerRow, cx: &App) -> Indexer {
        let value = |input: &Entity<InputState>| input.read(cx).value().trim().to_string();
        Indexer {
            id: row.id.clone(),
            kind: row.kind,
            name: value(&row.name),
            url: value(&row.url),
            api_key: value(&row.key),
            has_key: false,
            enabled: row.enabled,
        }
    }

    /// Saves every indexer; a blank key keeps the stored one.
    fn save_indexers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut settings = self.settings(cx);
        settings.indexers = self
            .indexers
            .iter()
            .map(|row| Self::indexer_from_row(row, cx))
            .collect();
        self.save(settings, window, cx);
    }

    fn add_indexer(&mut self, kind: IndexerKind, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.indexer_row(&Indexer::new(kind), window, cx);
        let focus = row.key.focus_handle(cx);
        self.indexers.push(row);
        self.save_indexers(window, cx);
        window.focus(&focus, cx);
    }

    fn remove_indexer(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.indexers.retain(|row| row.id != id);
        self.save_indexers(window, cx);
    }

    fn test_indexer(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.indexers.iter_mut().find(|row| row.id == id) else {
            return;
        };
        row.testing = true;
        row.test = None;
        let indexer = Self::indexer_from_row(row, cx);
        let id = id.to_string();
        let job = self
            .engine
            .read(cx)
            .run(move |core| async move { core.test_indexer(indexer).await });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await.map_err(|err| format!("{err:#}"));
            let _ = this.update(cx, |this, cx| {
                if let Some(row) = this.indexers.iter_mut().find(|row| row.id == id) {
                    row.testing = false;
                    row.test = Some(result);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn settings(&self, cx: &App) -> Settings {
        self.engine.read(cx).core().settings()
    }

    fn value(&self, field: Field, cx: &App) -> String {
        self.inputs[&field].read(cx).value().trim().to_string()
    }

    fn commit(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.value(field, cx);
        if field == Field::OnFinish {
            if value != self.settings(cx).on_finish
                && let Err(err) = self.engine.read(cx).core().set_on_finish(value)
            {
                window.push_notification(Notification::error(format!("{err:#}")), cx);
            }
            cx.notify();
            return;
        }
        let mut settings = self.settings(cx);
        match apply(field, &value, &mut settings) {
            Ok(()) => {
                self.errors.remove(&field);
                self.save(settings, window, cx);
            }
            Err(message) => {
                self.errors.insert(field, message);
            }
        }
        cx.notify();
    }

    fn save(&mut self, settings: Settings, window: &mut Window, cx: &mut Context<Self>) {
        if settings == self.settings(cx) {
            return;
        }
        let job = self
            .engine
            .read(cx)
            .run(move |core| async move { core.update_settings(settings).await });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |_, window, cx| {
                if let Err(err) = result {
                    window.push_notification(Notification::error(format!("{err:#}")), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle(
        &mut self,
        change: impl FnOnce(&mut Settings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.settings(cx);
        change(&mut settings);
        self.save(settings, window, cx);
        cx.notify();
    }

    fn browse(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("use this folder".into()),
        });
        let input = self.inputs[&field].clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                input.update(cx, |state, cx| {
                    state.set_value(path.to_string_lossy().into_owned(), window, cx)
                });
                this.commit(field, window, cx);
            });
        })
        .detach();
    }

    fn set_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = self.password.read(cx).value().to_string();
        if password.chars().count() < 8 {
            window.push_notification(Notification::warning("use at least 8 characters"), cx);
            return;
        }
        self.password
            .update(cx, |state, cx| state.set_value("", window, cx));
        let job = self.engine.read(cx).run(move |core| async move {
            let hash = bawkbit_web::hash_password(&password)?;
            core.set_web_password_hash(hash)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |_, window, cx| {
                let note = match result {
                    Ok(()) => Notification::success("password saved"),
                    Err(err) => Notification::error(format!("{err:#}")),
                };
                window.push_notification(note, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn export_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folder = dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let picked = cx.prompt_for_new_path(&folder, Some("bawkbit-backup.json"));
        let backup = self.engine.read(cx).core().backup();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let written = serde_json::to_vec_pretty(&backup)
                .map_err(std::io::Error::other)
                .and_then(|json| std::fs::write(&path, json));
            let _ = cx.update(|window, cx| {
                let note = match written {
                    Ok(()) => Notification::success(format!("saved {}", path.display())),
                    Err(err) => {
                        Notification::error(format!("cannot save {}: {err}", path.display()))
                    }
                };
                window.push_notification(note, cx);
            });
        })
        .detach();
    }

    fn import_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("restore".into()),
        });
        let engine = self.engine.clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let Ok(job) = cx.update(|_, cx| {
                engine.read(cx).run(move |core| async move {
                    let bytes = std::fs::read(&path)?;
                    let backup: Backup = serde_json::from_slice(&bytes)
                        .map_err(|_| anyhow::anyhow!("that file is not a bawkbit backup"))?;
                    core.restore(backup).await
                })
            }) else {
                return;
            };
            let result = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let note = match result {
                    Ok(()) => {
                        this.reload(window, cx);
                        Notification::success("settings restored")
                    }
                    Err(err) => Notification::error(format!("{err:#}")),
                };
                window.push_notification(note, cx);
            });
        })
        .detach();
    }

    /// Puts the engine's settings back in every box, after a restore changed them underneath.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = self.settings(cx);
        for (field, value) in field_values(&s) {
            if let Some(input) = self.inputs.get(&field) {
                input.update(cx, |state, cx| state.set_value(value, window, cx));
            }
        }
        self.errors.clear();
        self.indexers = s
            .indexers
            .iter()
            .map(|indexer| self.indexer_row(indexer, window, cx))
            .collect();
        cx.notify();
    }

    fn pick_theme(&mut self, id: SharedString, mode: Mode, cx: &mut Context<Self>) {
        self.theme = id.clone();
        self.mode = mode;
        cx.emit(SettingsEvent::Theme(id, mode));
        cx.notify();
    }

    fn text(&self, field: Field, p: &Palette) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(kit::input(&self.inputs[&field]))
            .when_some(self.errors.get(&field), |this, error| {
                this.child(div().text_size(px(12.)).text_color(p.danger).child(*error))
            })
    }

    fn folder(&self, field: Field, p: &Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .gap_2()
            .child(div().flex_1().child(self.text(field, p)))
            .child(
                kit::icon_button(
                    SharedString::from(format!("browse-{field:?}")),
                    IconName::FolderOpen,
                    "pick a folder",
                )
                .on_click(cx.listener(move |this, _, window, cx| this.browse(field, window, cx))),
            )
    }

    fn switch(
        &self,
        id: &'static str,
        on: bool,
        change: impl Fn(&mut Settings, bool) + 'static,
        cx: &mut Context<Self>,
    ) -> Switch {
        Switch::new(id).checked(on).on_click(cx.listener(
            move |this, checked: &bool, window, cx| {
                let checked = *checked;
                this.toggle(|s| change(s, checked), window, cx)
            },
        ))
    }

    fn render_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let update = self.updates.read(cx).has_news();
        div()
            .w(px(170.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .children(Tab::ALL.map(|tab| {
                let active = tab == self.tab;
                div()
                    .id(tab.label())
                    .h(px(32.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(if active { p.text_strong } else { p.text_weak })
                    .when(active, |this| this.bg(p.bg_hover))
                    .hover(|style| style.text_color(p.text_strong))
                    .child(Icon::new(tab.icon()).size(px(15.)))
                    .child(div().flex_1().child(tab.label()))
                    .when(tab == Tab::About && update, |this| {
                        this.child(kit::dot(p.yolk))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
            }))
    }

    fn render_downloads(&self, s: &Settings, p: &Palette, cx: &mut Context<Self>) -> Div {
        let panel = panel()
            .child(field(
                "download folder",
                "",
                self.folder(Field::DownloadDir, p, cx),
                p,
            ))
            .child(field(
                "watch folder",
                ".torrent files put here are added on their own",
                self.folder(Field::WatchDir, p, cx),
                p,
            ))
            .child(field(
                "start paused",
                "",
                self.switch(
                    "start-paused",
                    s.start_paused,
                    |s, on| s.start_paused = on,
                    cx,
                ),
                p,
            ))
            .child(field(
                "add magnets without asking",
                "skips the file list for magnet links",
                Switch::new("quick-add")
                    .checked(self.quick_add)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.quick_add = *checked;
                        cx.emit(SettingsEvent::QuickAdd(*checked));
                        cx.notify();
                    })),
                p,
            ))
            .child(field(
                "when one finishes, run",
                "%N name, %F file or folder, %D its folder, %L category, %G tags, %Z size, %C file count, %I id",
                self.text(Field::OnFinish, p),
                p,
            ))
            .child(field(
                "check rss feeds every",
                "minutes",
                self.text(Field::RssMinutes, p),
                p,
            ));
        #[cfg(windows)]
        let panel = {
            let handled = crate::instance::links::handled_here();
            let control: AnyElement = if handled {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(p.text_weak)
                    .child(
                        Icon::new(IconName::CircleCheck)
                            .size(px(14.))
                            .text_color(p.success),
                    )
                    .child("bawkbit opens them")
                    .into_any_element()
            } else {
                kit::button("take-links", cx)
                    .small()
                    .label("open them with bawkbit")
                    .on_click(cx.listener(|_, _, window, cx| {
                        if let Err(err) = crate::instance::links::take_over() {
                            window.push_notification(Notification::error(err), cx);
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            };
            panel.child(field("magnet links and .torrent files", "", control, p))
        };
        panel
    }

    fn render_connection(&self, s: &Settings, p: &Palette, cx: &mut Context<Self>) -> Div {
        let port = self.engine.read(cx).snapshot.totals.port;
        let port_note = match port {
            Some(port) if port == s.listen_port => String::new(),
            Some(port) => format!("listening on {port}"),
            None => "not listening: is another app using this port?".into(),
        };
        panel()
            .child(field(
                "listening port",
                port_note,
                self.text(Field::Port, p),
                p,
            ))
            .child(field(
                "upnp",
                "asks your router to open the port",
                self.switch("upnp", s.upnp, |s, on| s.upnp = on, cx),
                p,
            ))
            .child(field(
                "dht",
                "finds peers without trackers",
                self.switch("dht", s.dht, |s, on| s.dht = on, cx),
                p,
            ))
            .child(field(
                "utp",
                "also connects over utp, which the engine still calls new",
                self.switch("utp", s.utp, |s, on| s.utp = on, cx),
                p,
            ))
            .child(field(
                "peers per torrent",
                "",
                self.text(Field::PeerLimit, p),
                p,
            ))
    }

    fn render_speed(&self, s: &Settings, p: &Palette, cx: &mut Context<Self>) -> Div {
        let days = div().flex().gap_1().children(
            ["mo", "tu", "we", "th", "fr", "sa", "su"]
                .into_iter()
                .enumerate()
                .map(|(ix, label)| {
                    let on = s.schedule.days & (1 << ix) != 0;
                    kit::chip(SharedString::from(format!("day-{ix}")), label, on, p).on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.toggle(|s| s.schedule.days ^= 1 << ix, window, cx)
                        }),
                    )
                }),
        );
        let pair = |a: Field, b: Field, icons: (IconName, IconName)| {
            div()
                .flex()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(icons.0).size(px(14.)).text_color(p.blue))
                        .child(div().flex_1().child(self.text(a, p))),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(icons.1).size(px(14.)).text_color(p.green))
                        .child(div().flex_1().child(self.text(b, p))),
                )
        };
        let arrows = (IconName::ArrowDown, IconName::ArrowUp);
        panel()
            .child(field(
                "limits",
                "per second, like 500 KB or 2 MB",
                pair(Field::DownLimit, Field::UpLimit, arrows),
                p,
            ))
            .child(field(
                "alternative limits",
                "the turtle in the title bar switches to these",
                pair(Field::AltDown, Field::AltUp, arrows),
                p,
            ))
            .child(field(
                "schedule",
                "uses the alternative limits at these times",
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(self.switch(
                        "schedule",
                        s.schedule.enabled,
                        |s, on| s.schedule.enabled = on,
                        cx,
                    ))
                    .when(s.schedule.enabled, |this| {
                        this.child(pair(
                            Field::From,
                            Field::To,
                            (IconName::Clock, IconName::Clock),
                        ))
                        .child(days)
                    }),
                p,
            ))
    }

    fn render_search(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let cards: Vec<_> = self
            .indexers
            .iter()
            .map(|row| {
                let (test_id, remove_id, toggle_id) =
                    (row.id.clone(), row.id.clone(), row.id.clone());
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.border_weak)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(72.))
                                    .flex_none()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text_strong)
                                    .child(row.kind.label()),
                            )
                            .child(
                                div()
                                    .w(px(180.))
                                    .flex_none()
                                    .child(kit::input(&row.name).small()),
                            )
                            .child(div().flex_1())
                            .child(
                                Switch::new(SharedString::from(format!("indexer-on-{}", row.id)))
                                    .checked(row.enabled)
                                    .tooltip("search this one")
                                    .on_click(cx.listener(
                                        move |this, checked: &bool, window, cx| {
                                            if let Some(row) = this
                                                .indexers
                                                .iter_mut()
                                                .find(|row| row.id == toggle_id)
                                            {
                                                row.enabled = *checked;
                                            }
                                            this.save_indexers(window, cx);
                                            cx.notify();
                                        },
                                    )),
                            )
                            .child(
                                kit::button(
                                    SharedString::from(format!("indexer-test-{}", row.id)),
                                    cx,
                                )
                                .small()
                                .label("test")
                                .loading(row.testing)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.test_indexer(&test_id, window, cx)
                                    },
                                )),
                            )
                            .child(
                                kit::icon_button(
                                    SharedString::from(format!("indexer-remove-{}", row.id)),
                                    IconName::Trash,
                                    "remove",
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.remove_indexer(&remove_id, window, cx)
                                    },
                                )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(div().flex_1().child(kit::input(&row.url).small()))
                            .child(
                                div()
                                    .w(px(280.))
                                    .flex_none()
                                    .child(kit::input(&row.key).small().mask_toggle()),
                            ),
                    )
                    .when_some(row.test.clone(), |this, test| {
                        let (color, text) = match test {
                            Ok(message) => (p.success, message),
                            Err(error) => (p.danger, error),
                        };
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_size(px(12.))
                                .text_color(color)
                                .child(kit::dot(color))
                                .child(text),
                        )
                    })
            })
            .collect();
        panel()
            .max_w(px(820.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(p.text_weak)
                    .child("search asks these. the api key is under settings, general in prowlarr, and at the top right of jackett's dashboard."),
            )
            .children(cards)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        kit::button("add-prowlarr", cx)
                            .small()
                            .icon(Icon::new(IconName::Plus))
                            .label("add prowlarr")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_indexer(IndexerKind::Prowlarr, window, cx)
                            })),
                    )
                    .child(
                        kit::button("add-jackett", cx)
                            .small()
                            .icon(Icon::new(IconName::Plus))
                            .label("add jackett")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_indexer(IndexerKind::Jackett, window, cx)
                            })),
                    ),
            )
    }

    fn render_queue(&self, p: &Palette) -> Div {
        panel()
            .child(field(
                "downloads at once",
                "the rest wait their turn",
                self.text(Field::MaxActive, p),
                p,
            ))
            .child(field(
                "stop seeding at ratio",
                "uploaded divided by downloaded",
                self.text(Field::Ratio, p),
                p,
            ))
            .child(field(
                "stop seeding after",
                "minutes",
                self.text(Field::SeedMinutes, p),
                p,
            ))
    }

    fn render_web(&self, s: &Settings, p: &Palette, cx: &mut Context<Self>) -> Div {
        let status = self.engine.read(cx).web_status();
        let (color, text, url) = match &status {
            WebStatus::Listening(addr) => {
                let host = if addr.ip().is_unspecified() {
                    "localhost".to_string()
                } else {
                    addr.ip().to_string()
                };
                let url = format!("http://{host}:{}", addr.port());
                (p.success, url.clone(), Some(url))
            }
            WebStatus::Failed(err) => (p.danger, err.clone(), None),
            WebStatus::Off => (p.text_weaker, "off".to_string(), None),
        };
        let has_password = !s.web.password_hash.is_empty();
        panel()
            .child(field(
                "web ui",
                "control bawkbit from a browser on any device",
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(self.switch("web", s.web.enabled, |s, on| s.web.enabled = on, cx))
                    .child(kit::dot(color))
                    .child(kit::truncate(text).text_color(p.text_weak))
                    .when_some(url, |this, url| {
                        this.child(
                            kit::icon_button(
                                "open-web",
                                IconName::ExternalLink,
                                "open in a browser",
                            )
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                    }),
                p,
            ))
            .child(field(
                "address and port",
                "0.0.0.0 lets other devices in; 127.0.0.1 only this one",
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1().child(self.text(Field::WebAddress, p)))
                    .child(
                        div()
                            .w(px(110.))
                            .flex_none()
                            .child(self.text(Field::WebPort, p)),
                    ),
                p,
            ))
            .child(field("name", "", self.text(Field::WebUser, p), p))
            .child(field(
                "password",
                if has_password {
                    "a password is set"
                } else {
                    "needed before anyone can log in"
                },
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .child(kit::input(&self.password).mask_toggle()),
                    )
                    .child(kit::button("set-password", cx).label("save").on_click(
                        cx.listener(|this, _, window, cx| this.set_password(window, cx)),
                    )),
                p,
            ))
            .child(field(
                "skip login on this computer",
                "for browsers here; never for other devices or proxies",
                self.switch(
                    "local-bypass",
                    s.web.local_bypass,
                    |s, on| s.web.local_bypass = on,
                    cx,
                ),
                p,
            ))
    }

    fn render_app(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let config = cx.global::<Config>().clone();
        let config_switch = |id: &'static str, on: bool, change: fn(&mut Config, bool)| {
            Switch::new(id)
                .checked(on)
                .on_click(move |checked: &bool, _, cx| {
                    let checked = *checked;
                    Config::edit(cx, |config| change(config, checked));
                })
        };
        let panel = panel()
            .child(field(
                "tray icon",
                "pause, resume and open bawkbit from the tray",
                config_switch("tray", config.tray, |c, on| c.tray = on),
                p,
            ))
            .child(field(
                "keep running when closed",
                "closing the window leaves downloads and seeding going",
                config_switch("close-to-tray", config.close_to_tray, |c, on| {
                    c.close_to_tray = on
                })
                .disabled(!config.tray),
                p,
            ))
            .child(field(
                "notify when done",
                "a notification for each finished download",
                config_switch("notify-finished", config.notify_finished, |c, on| {
                    c.notify_finished = on
                }),
                p,
            ));
        if !autostart::supported() {
            return panel;
        }
        panel.child(field(
            "start at login",
            if config.tray {
                "starts in the tray"
            } else {
                ""
            },
            Switch::new("start-at-login")
                .checked(self.start_at_login)
                .on_click(cx.listener(|this, checked: &bool, window, cx| {
                    match autostart::set(*checked) {
                        Ok(()) => this.start_at_login = *checked,
                        Err(err) => window.push_notification(Notification::error(err), cx),
                    }
                    cx.notify();
                })),
            p,
        ))
    }

    fn render_appearance(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let own = self.theme == DEFAULT_THEME;
        let modes = kit::segmented(p).children(
            [
                (Mode::System, "system"),
                (Mode::Dark, "dark"),
                (Mode::Light, "light"),
            ]
            .map(|(mode, label)| {
                kit::segment(label, label, own && self.mode == mode, p).on_click(cx.listener(
                    move |this, _, _, cx| this.pick_theme(DEFAULT_THEME.into(), mode, cx),
                ))
            }),
        );
        let looks = theme::looks(cx).to_vec();
        let cards = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(looks.into_iter().map(|look| {
                let active = look.id == self.theme;
                let (bg, fg, cursor) = if look.id == DEFAULT_THEME && active {
                    (p.bg, p.text_strong, p.yolk)
                } else {
                    (look.bg, look.fg, look.cursor)
                };
                look_card(&look, bg, fg, cursor, active, p).on_click(cx.listener({
                    let id = look.id.clone();
                    move |this, _, _, cx| {
                        let mode = this.mode;
                        this.pick_theme(id.clone(), mode, cx)
                    }
                }))
            }));
        panel().child(modes).child(cards)
    }

    fn render_update(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let state = self.updates.read(cx).state.clone();
        let (icon, color, text): (IconName, Hsla, String) = match &state {
            UpdateState::Idle => (IconName::RefreshCw, p.icon, "not checked yet".into()),
            UpdateState::Checking => (IconName::RefreshCw, p.icon, "checking for updates…".into()),
            UpdateState::Current => (
                IconName::CircleCheck,
                p.success,
                "you have the newest version".into(),
            ),
            UpdateState::Available(release) => (
                IconName::Download,
                p.yolk,
                format!("bawkbit {} is out", release.version),
            ),
            UpdateState::Downloading {
                version,
                done,
                total,
            } => (
                IconName::Download,
                p.yolk,
                format!(
                    "downloading {version}… {} of {}",
                    format::bytes(*done),
                    format::bytes(*total)
                ),
            ),
            UpdateState::Ready { version, .. } => (
                IconName::CircleCheck,
                p.success,
                format!("{version} is installed. restart to use it."),
            ),
            UpdateState::Failed(error) => (IconName::CircleAlert, p.danger, error.clone()),
        };
        let blocked = match &state {
            UpdateState::Available(_) => crate::updater::install_path().err(),
            _ => None,
        };
        let updates = self.updates.clone();
        let action = match &state {
            UpdateState::Idle | UpdateState::Current | UpdateState::Failed(_) => Some(
                kit::button("check-updates-now", cx)
                    .small()
                    .label(if matches!(state, UpdateState::Failed(_)) {
                        "try again"
                    } else {
                        "check now"
                    })
                    .on_click(move |_, _, cx| updates.update(cx, |updates, cx| updates.check(cx))),
            ),
            UpdateState::Available(_) if blocked.is_none() => Some(
                Button::new("install-update")
                    .primary()
                    .small()
                    .label("update")
                    .on_click(move |_, _, cx| {
                        updates.update(cx, |updates, cx| updates.install(cx))
                    }),
            ),
            UpdateState::Ready { .. } => Some(
                Button::new("restart-update")
                    .primary()
                    .small()
                    .label("restart now")
                    .on_click(move |_, _, cx| {
                        updates.update(cx, |updates, cx| updates.restart(cx))
                    }),
            ),
            _ => None,
        };
        let notes = match &state {
            UpdateState::Available(release) => Some(release.page.clone()),
            _ => None,
        };
        let fraction = match &state {
            UpdateState::Downloading { done, total, .. } => {
                Some((*done as f32 / (*total).max(1) as f32).min(1.0))
            }
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded(px(6.))
            .border_1()
            .border_color(p.border_weak)
            .bg(p.bg_weak)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(Icon::new(icon).size(px(16.)).text_color(color))
                    .child(div().flex_1().min_w_0().text_color(p.text).child(text))
                    .children(notes.map(|page| {
                        kit::button("release-notes", cx)
                            .small()
                            .icon(Icon::new(IconName::ExternalLink))
                            .label("what's new")
                            .on_click(move |_, _, cx| cx.open_url(&page))
                    }))
                    .children(action),
            )
            .children(blocked.map(|reason| {
                div()
                    .text_size(px(12.))
                    .text_color(p.text_weak)
                    .child(reason)
            }))
            .children(fraction.map(|fraction| kit::progress_bar(fraction, p.yolk, p)))
    }

    fn render_about(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let home = bawkbit_core::data_dir();
        panel()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        svg()
                            .path(crate::assets::MARK)
                            .size(px(28.))
                            .text_color(p.text_strong),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_color(p.text_strong)
                                    .child(format!("bawkbit {}", bawkbit_core::VERSION)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(p.text_weak)
                                    .child("a torrent client"),
                            ),
                    ),
            )
            .child(self.render_update(p, cx))
            .child(field(
                "check for updates",
                "asks github once a day",
                Switch::new("check-updates")
                    .checked(self.updates.read(cx).automatic())
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        let on = *checked;
                        this.updates
                            .update(cx, |updates, cx| updates.set_automatic(on, cx));
                        cx.emit(SettingsEvent::CheckUpdates(on));
                    })),
                p,
            ))
            .child(field(
                "settings and session",
                "",
                kit::button("open-home", cx)
                    .small()
                    .icon(Icon::new(IconName::FolderOpen))
                    .label(SharedString::from(home.to_string_lossy().into_owned()))
                    .on_click(move |_, _, cx| cx.open_with_system(&home)),
                p,
            ))
            .child(field(
                "backup",
                "settings, categories, tags and rss, without passwords or api keys",
                div()
                    .flex()
                    .gap_2()
                    .child(
                        kit::button("export-backup", cx)
                            .small()
                            .icon(Icon::new(IconName::FileDown))
                            .label("export")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.export_backup(window, cx)),
                            ),
                    )
                    .child(
                        kit::button("import-backup", cx)
                            .small()
                            .icon(Icon::new(IconName::FileUp))
                            .label("import")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.import_backup(window, cx)),
                            ),
                    ),
                p,
            ))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(p.text_weaker)
                    .child("made with rust and gpui kit. torrents run on the librqbit engine."),
            )
    }
}

fn apply(field: Field, value: &str, s: &mut Settings) -> Result<(), &'static str> {
    let count = |value: &str| -> Result<u32, &'static str> {
        if value.is_empty() {
            Ok(0)
        } else {
            value.parse().map_err(|_| "use a whole number")
        }
    };
    let rate = |value: &str| format::parse_rate(value).ok_or("use a speed like 500 KB or 2 MB");
    let port = |value: &str| {
        value
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or("use a port from 1 to 65535")
    };
    match field {
        Field::DownloadDir if value.is_empty() => return Err("pick a folder"),
        Field::DownloadDir => s.download_dir = value.into(),
        Field::WatchDir => s.watch_dir = value.into(),
        Field::Port => s.listen_port = port(value)?,
        Field::PeerLimit => s.peer_limit = count(value)?,
        Field::DownLimit => s.down_limit = rate(value)?,
        Field::UpLimit => s.up_limit = rate(value)?,
        Field::AltDown => s.alt_down_limit = rate(value)?,
        Field::AltUp => s.alt_up_limit = rate(value)?,
        Field::From => {
            s.schedule.from = format::parse_clock(value).ok_or("use a time like 08:00")?
        }
        Field::To => s.schedule.to = format::parse_clock(value).ok_or("use a time like 20:00")?,
        Field::MaxActive => s.max_active_downloads = count(value)?,
        Field::Ratio => {
            s.ratio_limit = if value.is_empty() {
                0.0
            } else {
                value
                    .parse::<f32>()
                    .ok()
                    .filter(|ratio| *ratio >= 0.0)
                    .ok_or("use a number like 2 or 1.5")?
            }
        }
        Field::SeedMinutes => s.seed_minutes = count(value)?,
        Field::WebAddress => {
            s.web.address = if value.is_empty() {
                "0.0.0.0".into()
            } else {
                value.into()
            }
        }
        Field::WebPort => s.web.port = port(value)?,
        Field::WebUser if value.is_empty() => return Err("pick a name"),
        Field::WebUser => s.web.username = value.into(),
        Field::OnFinish => s.on_finish = value.into(),
        Field::RssMinutes => s.rss_minutes = count(value)?,
    }
    Ok(())
}

fn field_values(s: &Settings) -> Vec<(Field, String)> {
    vec![
        (Field::DownloadDir, s.download_dir.clone()),
        (Field::WatchDir, s.watch_dir.clone()),
        (Field::Port, s.listen_port.to_string()),
        (Field::PeerLimit, nonzero(s.peer_limit)),
        (Field::DownLimit, format::rate_input(s.down_limit)),
        (Field::UpLimit, format::rate_input(s.up_limit)),
        (Field::AltDown, format::rate_input(s.alt_down_limit)),
        (Field::AltUp, format::rate_input(s.alt_up_limit)),
        (Field::From, format::clock(s.schedule.from)),
        (Field::To, format::clock(s.schedule.to)),
        (Field::MaxActive, nonzero(s.max_active_downloads)),
        (
            Field::Ratio,
            if s.ratio_limit > 0.0 {
                s.ratio_limit.to_string()
            } else {
                String::new()
            },
        ),
        (Field::SeedMinutes, nonzero(s.seed_minutes)),
        (Field::WebAddress, s.web.address.clone()),
        (Field::WebPort, s.web.port.to_string()),
        (Field::WebUser, s.web.username.clone()),
        (Field::OnFinish, s.on_finish.clone()),
        (Field::RssMinutes, nonzero(s.rss_minutes)),
    ]
}

fn placeholder(field: Field) -> &'static str {
    match field {
        Field::DownloadDir => "",
        Field::WatchDir => "off",
        Field::Port => "",
        Field::PeerLimit => "engine default",
        Field::DownLimit => "no limit",
        Field::UpLimit => "no limit",
        Field::AltDown => "no limit",
        Field::AltUp => "no limit",
        Field::From => "08:00",
        Field::To => "20:00",
        Field::MaxActive => "no limit",
        Field::Ratio => "seed forever",
        Field::SeedMinutes => "seed forever",
        Field::WebAddress => "0.0.0.0",
        Field::WebPort => "",
        Field::WebUser => "admin",
        Field::OnFinish => "nothing",
        Field::RssMinutes => "only when asked",
    }
}

fn nonzero(value: u32) -> String {
    if value == 0 {
        String::new()
    } else {
        value.to_string()
    }
}

fn panel() -> Div {
    div().flex().flex_col().gap_5()
}

fn field(
    label: &'static str,
    hint: impl Into<SharedString>,
    control: impl IntoElement,
    p: &Palette,
) -> Div {
    let hint = hint.into();
    div()
        .flex()
        .gap_6()
        .child(
            div()
                .w(px(220.))
                .flex_none()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_color(p.text).child(label))
                .when(!hint.is_empty(), |this| {
                    this.child(div().text_size(px(12.)).text_color(p.text_weak).child(hint))
                }),
        )
        .child(div().flex_1().min_w_0().max_w(px(560.)).child(control))
}

fn look_card(
    look: &Look,
    bg: Hsla,
    fg: Hsla,
    cursor: Hsla,
    active: bool,
    p: &Palette,
) -> Stateful<Div> {
    let mut words: Vec<&str> = look.label.split(' ').collect();
    let last = words.pop().unwrap_or_default().to_string();
    div()
        .id(SharedString::from(format!("look-{}", look.id)))
        .w(px(150.))
        .flex()
        .flex_col()
        .gap_2()
        .p(px(10.))
        .rounded(px(4.))
        .border_2()
        .border_color(if active { p.yolk } else { p.border_weak })
        .cursor_pointer()
        .bg(bg)
        .text_color(fg)
        .text_size(px(12.))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x(px(7.))
                .children(words.into_iter().map(|word| div().child(word.to_string())))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(last)
                        .child(div().w(px(7.)).h(px(13.)).bg(cursor)),
                ),
        )
        .child(
            div().flex().gap(px(3.)).children(
                look.swatches
                    .map(|color| div().size(px(12.)).rounded(px(2.)).bg(color)),
            ),
        )
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let s = self.settings(cx);
        let body = match self.tab {
            Tab::Downloads => self.render_downloads(&s, &p, cx),
            Tab::Connection => self.render_connection(&s, &p, cx),
            Tab::Speed => self.render_speed(&s, &p, cx),
            Tab::Queue => self.render_queue(&p),
            Tab::Search => self.render_search(&p, cx),
            Tab::Web => self.render_web(&s, &p, cx),
            Tab::App => self.render_app(&p, cx),
            Tab::Appearance => self.render_appearance(&p, cx),
            Tab::About => self.render_about(&p, cx),
        };
        div()
            .size_full()
            .flex()
            .gap_8()
            .px(px(32.))
            .pt(px(28.))
            .child(self.render_tabs(&p, cx))
            .child(
                div()
                    .id("settings-panel")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .pb_8()
                    .child(body),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Field, Settings, apply};

    #[test]
    fn fields_parse_into_settings() {
        let mut s = Settings::default();
        apply(Field::DownLimit, "2 MB", &mut s).unwrap();
        assert_eq!(s.down_limit, 2 * 1024 * 1024);
        apply(Field::Ratio, "", &mut s).unwrap();
        assert_eq!(s.ratio_limit, 0.0);
        assert!(apply(Field::Port, "0", &mut s).is_err());
        assert!(apply(Field::DownloadDir, "", &mut s).is_err());
        apply(Field::WebAddress, "", &mut s).unwrap();
        assert_eq!(s.web.address, "0.0.0.0");
        apply(Field::From, "22:30", &mut s).unwrap();
        assert_eq!(s.schedule.from, 22 * 60 + 30);
    }
}
