use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use bawkbit_core::{Action, Id, State, Torrent};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    Icon, Side, Sizable, VirtualListScrollHandle, WindowExt, v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::chrome::run_action;
use super::filter::{Filters, Status, state_icon, state_label};
use super::{act, kit, prompts};
use crate::config::{Column, Config};
use crate::engine::Engine;
use crate::format;
use crate::theme::{Palette, palette};

const ROW: f32 = 46.;

pub enum ListEvent {
    Focus(Option<Id>),
    Sorted(Column, bool),
    Add,
}

struct Rows {
    rev: u64,
    key: (Filters, Column, bool),
    torrents: Rc<Vec<Torrent>>,
    sizes: Rc<Vec<Size<Pixels>>>,
}

pub struct TorrentList {
    engine: Entity<Engine>,
    pub filters: Filters,
    search: Entity<InputState>,
    sort: Column,
    descending: bool,
    selected: HashSet<Id>,
    anchor: Option<Id>,
    focused: Option<Id>,
    rows: Option<Rows>,
    scroll: VirtualListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ListEvent> for TorrentList {}

impl TorrentList {
    pub fn new(
        engine: Entity<Engine>,
        config: &Config,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("filter"));
        let subscriptions = vec![
            cx.observe(&engine, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.filters.text = input.read(cx).value().to_string();
                    cx.notify();
                }
            }),
        ];
        Self {
            engine,
            filters: Filters::default(),
            search,
            sort: config.sort,
            descending: config.descending,
            selected: HashSet::new(),
            anchor: None,
            focused: None,
            rows: None,
            scroll: VirtualListScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.filters.status = status;
        cx.notify();
    }

    pub fn toggle_category(&mut self, name: String, cx: &mut Context<Self>) {
        self.filters.category = (self.filters.category.as_ref() != Some(&name)).then_some(name);
        cx.notify();
    }

    pub fn toggle_tag(&mut self, tag: String, cx: &mut Context<Self>) {
        self.filters.tag = (self.filters.tag.as_ref() != Some(&tag)).then_some(tag);
        cx.notify();
    }

    /// Selects a just-added torrent once it shows up.
    pub fn reveal(&mut self, id: Id, cx: &mut Context<Self>) {
        self.filters = Filters {
            text: self.filters.text.clone(),
            ..Filters::default()
        };
        self.selected = HashSet::from([id.clone()]);
        self.anchor = Some(id.clone());
        self.set_focus(Some(id), cx);
    }

    pub fn clear_focus(&mut self, cx: &mut Context<Self>) {
        self.set_focus(None, cx);
    }

    fn set_focus(&mut self, id: Option<Id>, cx: &mut Context<Self>) {
        if self.focused != id {
            self.focused = id.clone();
            cx.emit(ListEvent::Focus(id));
        }
        cx.notify();
    }

    fn refresh(&mut self, cx: &App) -> Rc<Vec<Torrent>> {
        let snapshot = self.engine.read(cx).snapshot.clone();
        let key = (self.filters.clone(), self.sort, self.descending);
        if let Some(rows) = &self.rows
            && rows.rev == snapshot.rev
            && rows.key == key
        {
            return rows.torrents.clone();
        }
        let mut torrents: Vec<Torrent> = snapshot
            .torrents
            .iter()
            .filter(|torrent| self.filters.matches(torrent))
            .cloned()
            .collect();
        let (sort, descending) = (self.sort, self.descending);
        torrents.sort_by(|a, b| {
            let order =
                compare(sort, a, b).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            if descending { order.reverse() } else { order }
        });
        let torrents = Rc::new(torrents);
        let sizes = Rc::new(vec![size(px(1.), px(ROW)); torrents.len()]);
        let present: HashSet<&Id> = snapshot.torrents.iter().map(|t| &t.id).collect();
        self.selected.retain(|id| present.contains(id));
        self.rows = Some(Rows {
            rev: snapshot.rev,
            key,
            torrents: torrents.clone(),
            sizes,
        });
        torrents
    }

    fn selection(&self) -> Vec<Id> {
        let mut ids: Vec<Id> = self.selected.iter().cloned().collect();
        ids.sort();
        ids
    }

    fn click(&mut self, id: &Id, modifiers: Modifiers, count: usize, cx: &mut Context<Self>) {
        let rows = self.refresh(cx);
        if modifiers.shift {
            let anchor = self.anchor.clone().unwrap_or_else(|| id.clone());
            let a = rows.iter().position(|t| t.id == anchor);
            let b = rows.iter().position(|t| &t.id == id);
            if let (Some(a), Some(b)) = (a, b) {
                let (start, end) = (a.min(b), a.max(b));
                if !(modifiers.control || modifiers.platform) {
                    self.selected.clear();
                }
                self.selected
                    .extend(rows[start..=end].iter().map(|t| t.id.clone()));
            }
        } else if modifiers.control || modifiers.platform {
            if !self.selected.remove(id) {
                self.selected.insert(id.clone());
            }
            self.anchor = Some(id.clone());
        } else {
            self.selected = HashSet::from([id.clone()]);
            self.anchor = Some(id.clone());
            if count == 2
                && let Some(torrent) = rows.iter().find(|t| &t.id == id)
            {
                open_folder(&torrent.save_path, cx);
            }
        }
        self.set_focus(Some(id.clone()), cx);
    }

    /// First click sorts by a column, the second reverses it, the third goes back to newest first.
    fn sort_by(&mut self, column: Column, cx: &mut Context<Self>) {
        let natural = !matches!(column, Column::Name | Column::Eta);
        if self.sort != column {
            self.sort = column;
            self.descending = natural;
        } else if self.descending == natural {
            self.descending = !natural;
        } else {
            self.sort = Column::Added;
            self.descending = true;
        }
        cx.emit(ListEvent::Sorted(self.sort, self.descending));
        cx.notify();
    }

    /// Keys for the list; returns whether it used the key.
    pub fn on_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let command = keystroke.modifiers.control || keystroke.modifiers.platform;
        let ids = self.selection();
        match keystroke.key.as_str() {
            "a" if command => {
                let rows = self.refresh(cx);
                self.selected = rows.iter().map(|t| t.id.clone()).collect();
                cx.notify();
            }
            "f" if command => window.focus(&self.search.focus_handle(cx), cx),
            "escape" => {
                self.selected.clear();
                self.set_focus(None, cx);
            }
            "space" if !ids.is_empty() => {
                let snapshot = self.engine.read(cx).snapshot.clone();
                let any_running = snapshot
                    .torrents
                    .iter()
                    .any(|t| self.selected.contains(&t.id) && !t.state.is_paused());
                let action = if any_running {
                    Action::Pause { ids }
                } else {
                    Action::Resume { ids }
                };
                act(&self.engine, action, window, cx);
            }
            "delete" | "backspace" if !ids.is_empty() => {
                let delete_files = keystroke.modifiers.shift;
                prompts::remove_torrents(self.engine.clone(), ids, delete_files, window, cx);
            }
            "up" | "down" => {
                let rows = self.refresh(cx);
                if rows.is_empty() {
                    return true;
                }
                let current = self
                    .focused
                    .as_ref()
                    .and_then(|id| rows.iter().position(|t| &t.id == id));
                let next = match (current, keystroke.key.as_str()) {
                    (None, _) => 0,
                    (Some(ix), "up") => ix.saturating_sub(1),
                    (Some(ix), _) => (ix + 1).min(rows.len() - 1),
                };
                let id = rows[next].id.clone();
                self.selected = HashSet::from([id.clone()]);
                self.anchor = Some(id.clone());
                self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
                self.set_focus(Some(id), cx);
            }
            _ => return false,
        }
        true
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = palette(cx);
        let rows = self.refresh(cx);
        range
            .filter_map(|ix| rows.get(ix))
            .map(|torrent| self.row(torrent, &p, cx).into_any_element())
            .collect()
    }

    fn row(&self, t: &Torrent, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.contains(&t.id);
        let color = p.state(t.state);
        let active = !t.state.is_paused() && t.state != State::Queued;
        let id = t.id.clone();
        let menu_view = cx.entity().downgrade();
        let state_tip: SharedString = match &t.error {
            Some(error) => format!("{}: {error}", state_label(t.state)).into(),
            None => state_label(t.state).into(),
        };
        let peers = if t.peers > 0 {
            format!("{}/{}", t.seeds, t.peers)
        } else {
            String::new()
        };

        div()
            .id(SharedString::from(t.id.clone()))
            .w_full()
            .h(px(ROW))
            .px_3()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(p.border_weak)
            .text_size(px(12.))
            .text_color(p.text)
            .when(selected, |this| this.bg(p.yolk_dim))
            .when(!selected, |this| this.hover(|style| style.bg(p.bg_weak)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let id = id.clone();
                    move |this, event: &MouseDownEvent, _, cx| {
                        this.click(&id, event.modifiers, event.click_count, cx)
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = id.clone();
                    move |this, _, _, cx| {
                        if !this.selected.contains(&id) {
                            this.selected = HashSet::from([id.clone()]);
                            this.anchor = Some(id.clone());
                        }
                        this.set_focus(Some(id.clone()), cx);
                    }
                }),
            )
            .context_menu(move |menu, window, cx| row_menu(menu, menu_view.clone(), window, cx))
            .child(
                div()
                    .id(SharedString::from(format!("state-{}", t.id)))
                    .flex_none()
                    .child(
                        Icon::new(state_icon(t.state))
                            .size(px(14.))
                            .text_color(color),
                    )
                    .tooltip(move |window, cx| Tooltip::new(state_tip.clone()).build(window, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        kit::truncate(t.name.clone())
                            .text_size(px(13.))
                            .text_color(if active { p.text_strong } else { p.text }),
                    )
                    .child(kit::progress_bar(t.progress, color, p)),
            )
            .child(cell(format::bytes(t.size), 76., p.text_weak))
            .child(cell(format::percent(t.progress), 60., p.text_weak))
            .child(cell(format::speed(t.down), 88., p.text))
            .child(cell(format::speed(t.up), 88., p.text))
            .child(cell(format::eta(t.eta), 70., p.text_weak))
            .child(cell(format::ratio(t.ratio), 48., p.text_weak))
            .child(cell(peers, 56., p.text_weak))
    }

    fn header(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let column = |column: Column,
                      label: Option<&'static str>,
                      icon: Option<IconName>,
                      width: Option<f32>| {
            let active = self.sort == column;
            let arrow = if self.descending { "↓" } else { "↑" };
            div()
                .id(SharedString::from(format!("sort-{column:?}")))
                .flex()
                .items_center()
                .gap(px(3.))
                .map(|this| match width {
                    Some(width) => this.w(px(width)).flex_none().justify_end(),
                    None => this.flex_1().min_w_0(),
                })
                .cursor_pointer()
                .text_color(if active { p.text_strong } else { p.text_weaker })
                .hover(|style| style.text_color(p.text_strong))
                .when_some(icon, |this, icon| this.child(Icon::new(icon).size(px(12.))))
                .when_some(label, |this, label| this.child(label))
                .when(active, |this| this.child(arrow))
                .on_click(cx.listener(move |this, _, _, cx| this.sort_by(column, cx)))
        };
        div()
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(p.border_weak)
            .text_size(px(11.))
            .child(div().w(px(14.)).flex_none())
            .child(column(Column::Name, Some("name"), None, None))
            .child(column(Column::Size, Some("size"), None, Some(76.)))
            .child(column(Column::Progress, Some("done"), None, Some(60.)))
            .child(column(
                Column::Down,
                None,
                Some(IconName::ArrowDown),
                Some(88.),
            ))
            .child(column(Column::Up, None, Some(IconName::ArrowUp), Some(88.)))
            .child(column(Column::Eta, None, Some(IconName::Clock), Some(70.)))
            .child(column(
                Column::Ratio,
                None,
                Some(IconName::Share2),
                Some(48.),
            ))
            .child(column(
                Column::Peers,
                None,
                Some(IconName::Users),
                Some(56.),
            ))
    }

    fn footer(&self, rows: &[Torrent], p: &Palette, cx: &App) -> impl IntoElement {
        let snapshot = self.engine.read(cx).snapshot.clone();
        let totals = &snapshot.totals;
        let picked: Vec<&Torrent> = rows
            .iter()
            .filter(|t| self.selected.contains(&t.id))
            .collect();
        let info = |icon: IconName, text: String| {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(Icon::new(icon).size(px(12.)).text_color(p.icon))
                .child(text)
        };
        div()
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .border_t_1()
            .border_color(p.border_weak)
            .text_size(px(11.))
            .text_color(p.text_weak)
            .child(div().when(picked.len() > 1, |this| {
                let bytes: u64 = picked.iter().map(|t| t.size).sum();
                this.child(format!(
                    "{} selected · {}",
                    picked.len(),
                    format::bytes(bytes)
                ))
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .when_some(totals.free_space, |this, free| {
                        this.child(info(
                            IconName::HardDrive,
                            format!("{} free", format::bytes(free)),
                        ))
                    })
                    .child(info(
                        IconName::Network,
                        format!("{} nodes", format::count(totals.dht_nodes)),
                    ))
                    .when_some(totals.port, |this, port| {
                        this.child(info(IconName::Wifi, format!("port {port}")))
                    }),
            )
    }
}

fn compare(column: Column, a: &Torrent, b: &Torrent) -> Ordering {
    match column {
        Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        Column::Size => a.size.cmp(&b.size),
        Column::Progress => a.progress.total_cmp(&b.progress),
        Column::Down => a.down.cmp(&b.down),
        Column::Up => a.up.cmp(&b.up),
        Column::Eta => a.eta.unwrap_or(u64::MAX).cmp(&b.eta.unwrap_or(u64::MAX)),
        Column::Ratio => a.ratio.total_cmp(&b.ratio),
        Column::Peers => a.peers.cmp(&b.peers),
        Column::Added => a.added.cmp(&b.added),
    }
}

fn cell(text: String, width: f32, color: Hsla) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .flex()
        .justify_end()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(color)
        .child(text)
}

fn open_folder(path: &str, cx: &App) {
    let path = PathBuf::from(path);
    let target = if path.exists() {
        path
    } else {
        path.parent().map(PathBuf::from).unwrap_or(path)
    };
    cx.open_with_system(&target);
}

fn row_menu(
    menu: PopupMenu,
    view: WeakEntity<TorrentList>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(list) = view.upgrade() else {
        return menu;
    };
    let list = list.read(cx);
    let engine = list.engine.clone();
    let ids = list.selection();
    let snapshot = engine.read(cx).snapshot.clone();
    let picked: Vec<Torrent> = snapshot
        .torrents
        .iter()
        .filter(|t| ids.contains(&t.id))
        .cloned()
        .collect();
    if picked.is_empty() {
        return menu;
    }
    let any_paused = picked.iter().any(|t| t.state.is_paused());
    let any_running = picked.iter().any(|t| !t.state.is_paused());
    let all_forced = picked.iter().all(|t| t.forced);
    let all_quiet = picked.iter().all(|t| t.no_upload);
    let limits: HashSet<(u64, u64)> = picked.iter().map(|t| (t.down_limit, t.up_limit)).collect();
    let shared_limits = (limits.len() == 1).then(|| limits.into_iter().next().unwrap_or_default());
    let single = (picked.len() == 1).then(|| picked[0].clone());

    let item = |label: &'static str, icon: IconName, action: Action| {
        let engine = engine.clone();
        PopupMenuItem::new(label)
            .icon(Icon::new(icon))
            .on_click(move |_, window, cx| run_action(&engine, action.clone(), window, cx))
    };

    let mut menu = menu.min_w(px(200.)).check_side(Side::Right);
    if any_paused {
        menu = menu.item(item(
            "resume",
            IconName::Play,
            Action::Resume { ids: ids.clone() },
        ));
    }
    if any_running {
        menu = menu.item(item(
            "pause",
            IconName::Pause,
            Action::Pause { ids: ids.clone() },
        ));
    }
    let start = if all_forced {
        Action::Resume { ids: ids.clone() }
    } else {
        Action::ForceStart { ids: ids.clone() }
    };
    menu = menu
        .item(item("force start", IconName::FastForward, start).checked(all_forced))
        .item(item(
            "re-check",
            IconName::RefreshCw,
            Action::Recheck { ids: ids.clone() },
        ))
        .separator();

    let current_category = single.as_ref().and_then(|t| t.category.clone());
    let categories = snapshot.categories.clone();
    let (category_ids, category_engine) = (ids.clone(), engine.clone());
    menu = menu.submenu_with_icon(
        Some(Icon::new(IconName::Folder)),
        "category",
        window,
        cx,
        move |menu, _, _| {
            let pick = |label: String, category: Option<String>, checked: bool| {
                let (engine, ids) = (category_engine.clone(), category_ids.clone());
                PopupMenuItem::new(label)
                    .checked(checked)
                    .on_click(move |_, window, cx| {
                        let action = Action::SetCategory {
                            ids: ids.clone(),
                            category: category.clone(),
                        };
                        run_action(&engine, action, window, cx);
                    })
            };
            let mut menu = menu.item(pick("none".into(), None, current_category.is_none()));
            for category in &categories {
                let checked = current_category.as_ref() == Some(&category.name);
                menu = menu.item(pick(
                    category.name.clone(),
                    Some(category.name.clone()),
                    checked,
                ));
            }
            let (engine, ids) = (category_engine.clone(), category_ids.clone());
            menu.separator().item(
                PopupMenuItem::new("new category")
                    .icon(Icon::new(IconName::Plus))
                    .on_click(move |_, window, cx| {
                        let (engine, ids) = (engine.clone(), ids.clone());
                        ask(window, cx, "category name", move |name, window, cx| {
                            let action = Action::SetCategory {
                                ids: ids.clone(),
                                category: Some(name),
                            };
                            run_action(&engine, action, window, cx);
                        });
                    }),
            )
        },
    );

    let tags = snapshot.tags.clone();
    let (tag_ids, tag_engine, tag_picked) = (ids.clone(), engine.clone(), picked.clone());
    menu = menu.submenu_with_icon(
        Some(Icon::new(IconName::Tag)),
        "tags",
        window,
        cx,
        move |menu, _, _| {
            let mut menu = menu;
            for tag in &tags {
                let all = tag_picked.iter().all(|t| t.tags.contains(tag));
                let (engine, ids, tag) = (tag_engine.clone(), tag_ids.clone(), tag.clone());
                menu = menu.item(PopupMenuItem::new(tag.clone()).checked(all).on_click(
                    move |_, window, cx| {
                        let action = if all {
                            Action::RemoveTags {
                                ids: ids.clone(),
                                tags: vec![tag.clone()],
                            }
                        } else {
                            Action::AddTags {
                                ids: ids.clone(),
                                tags: vec![tag.clone()],
                            }
                        };
                        run_action(&engine, action, window, cx);
                    },
                ));
            }
            let (engine, ids) = (tag_engine.clone(), tag_ids.clone());
            menu.when(!tags.is_empty(), |menu| menu.separator()).item(
                PopupMenuItem::new("new tag")
                    .icon(Icon::new(IconName::Plus))
                    .on_click(move |_, window, cx| {
                        let (engine, ids) = (engine.clone(), ids.clone());
                        ask(window, cx, "tag", move |tag, window, cx| {
                            run_action(
                                &engine,
                                Action::AddTags {
                                    ids: ids.clone(),
                                    tags: vec![tag],
                                },
                                window,
                                cx,
                            );
                        });
                    }),
            )
        },
    );

    let quiet = Action::SetNoUpload {
        ids: ids.clone(),
        on: !all_quiet,
    };
    let (limit_engine, limit_ids) = (engine.clone(), ids.clone());
    menu = menu
        .item(item("no upload", IconName::Ban, quiet).checked(all_quiet))
        .item(
            PopupMenuItem::new("speed limits")
                .icon(Icon::new(IconName::Gauge))
                .on_click(move |_, window, cx| {
                    let (engine, ids) = (limit_engine.clone(), limit_ids.clone());
                    let action = move |down, up| Action::SetLimits {
                        ids: ids.clone(),
                        down,
                        up,
                    };
                    prompts::speed_limits(engine, shared_limits, action, window, cx)
                }),
        );
    if let Some(torrent) = &single {
        let (engine, id) = (engine.clone(), torrent.id.clone());
        menu = menu.item(
            PopupMenuItem::new("add trackers")
                .icon(Icon::new(IconName::ListPlus))
                .on_click(move |_, window, cx| {
                    prompts::add_trackers(engine.clone(), id.clone(), window, cx)
                }),
        );
    }

    menu = menu.separator();
    if let Some(torrent) = &single {
        let path = torrent.save_path.clone();
        menu = menu.item(
            PopupMenuItem::new("open folder")
                .icon(Icon::new(IconName::FolderOpen))
                .on_click(move |_, _, cx| open_folder(&path, cx)),
        );
    }
    let core = engine.read(cx).core().clone();
    menu = menu.submenu_with_icon(
        Some(Icon::new(IconName::Copy)),
        "copy",
        window,
        cx,
        move |menu, _, _| {
            let joined = |text: &dyn Fn(&Torrent) -> String| {
                picked.iter().map(text).collect::<Vec<_>>().join("\n")
            };
            let magnets = joined(&|t| {
                core.details(&t.id)
                    .map(|details| details.magnet)
                    .unwrap_or_else(|_| format!("magnet:?xt=urn:btih:{}", t.id))
            });
            let copy = |label: &'static str, text: String, note: &'static str| {
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    window.push_notification(note, cx);
                })
            };
            menu.item(copy("magnet link", magnets, "magnet link copied"))
                .item(copy(
                    "info hash",
                    joined(&|t| t.id.clone()),
                    "info hash copied",
                ))
                .item(copy("name", joined(&|t| t.name.clone()), "name copied"))
        },
    );

    let remove = |label: &'static str, icon: IconName, delete_files: bool| {
        let (engine, ids) = (engine.clone(), ids.clone());
        PopupMenuItem::new(label)
            .icon(Icon::new(icon))
            .on_click(move |_, window, cx| {
                prompts::remove_torrents(engine.clone(), ids.clone(), delete_files, window, cx)
            })
    };
    menu.separator()
        .item(remove("remove", IconName::X, false))
        .item(remove("remove and delete files", IconName::Trash, true))
}

/// A one-line prompt, such as for a new category name.
fn ask(
    window: &mut Window,
    cx: &mut App,
    placeholder: &'static str,
    on_submit: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
    let on_submit = Rc::new(on_submit);
    let submit = {
        let (input, on_submit) = (input.clone(), on_submit.clone());
        move |window: &mut Window, cx: &mut App| {
            let value = input.read(cx).value().trim().to_string();
            if !value.is_empty() {
                on_submit(value, window, cx);
                window.close_dialog(cx);
            }
        }
    };
    let submit = Rc::new(submit);
    let enter = submit.clone();
    let handle = window.window_handle();
    cx.subscribe(&input, move |_, event: &InputEvent, cx| {
        if matches!(event, InputEvent::PressEnter { .. }) {
            let enter = enter.clone();
            let _ = handle.update(cx, |_, window, cx| enter(window, cx));
        }
    })
    .detach();
    let focus = input.focus_handle(cx);
    window.open_dialog(cx, move |dialog, _, cx| {
        let submit = submit.clone();
        dialog.w(px(360.)).child(kit::input(&input)).footer(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    kit::button("cancel-ask", cx)
                        .label("cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new("ok-ask")
                        .primary()
                        .label("save")
                        .on_click(move |_, window, cx| submit(window, cx)),
                ),
        )
    });
    window.focus(&focus, cx);
}

impl Render for TorrentList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let rows = self.refresh(cx);
        let empty = self.engine.read(cx).snapshot.torrents.is_empty();
        let sizes = self
            .rows
            .as_ref()
            .map(|rows| rows.sizes.clone())
            .unwrap_or_default();

        let toolbar = div()
            .h(px(48.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(p.border_weak)
            .child(
                div().flex_1().min_w_0().max_w(px(420.)).child(
                    kit::input(&self.search)
                        .small()
                        .prefix(Icon::new(IconName::Search).size(px(14.)).text_color(p.icon))
                        .cleanable(true),
                ),
            )
            .child(div().flex_1())
            .child(
                Button::new("add")
                    .primary()
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .tooltip("add a torrent  ctrl+o, or paste a magnet link")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ListEvent::Add))),
            );

        let body = if empty {
            kit::empty_state(
                "drop a .torrent file here or paste a magnet link.",
                kit::button("add-empty", cx)
                    .label("add a torrent")
                    .icon(Icon::new(IconName::Plus))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ListEvent::Add))),
                &p,
            )
            .into_any_element()
        } else if rows.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(p.text_weaker)
                .child("nothing matches")
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    v_virtual_list(cx.entity(), "torrents", sizes, |this, range, _, cx| {
                        this.render_rows(range, cx)
                    })
                    .track_scroll(&self.scroll),
                )
                .vertical_scrollbar(&self.scroll)
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .when(!empty, |this| this.child(self.header(&p, cx)))
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .when(!empty, |this| this.child(self.footer(&rows, &p, cx)))
    }
}
