use std::cmp::Ordering;
use std::collections::HashSet;
use std::rc::Rc;

use bawkbit_core::{Id, SearchCategory, SearchReply, SearchRequest, SearchResult};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable, VirtualListScrollHandle, WindowExt, v_virtual_list};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::filter::Refine;
use super::kit;
use crate::engine::Engine;
use crate::format;
use crate::theme::{Palette, palette};

const ROW: f32 = 40.;

pub enum SearchEvent {
    Add(String),
    Reveal(Id),
    Configure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sort {
    Name,
    Size,
    Seeds,
    Peers,
    Age,
    Source,
}

enum Status {
    Idle,
    Searching,
    Done(SearchReply),
    Failed(String),
}

struct Rows {
    key: (u64, String, Sort, bool),
    results: Rc<Vec<SearchResult>>,
    sizes: Rc<Vec<Size<Pixels>>>,
}

pub struct SearchView {
    engine: Entity<Engine>,
    query: Entity<InputState>,
    refine: Entity<InputState>,
    category: SearchCategory,
    status: Status,
    generation: u64,
    sort: Sort,
    descending: bool,
    rows: Option<Rows>,
    scroll: VirtualListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SearchEvent> for SearchView {}

impl SearchView {
    pub fn new(engine: Entity<Engine>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("search"));
        let refine =
            cx.new(|cx| InputState::new(window, cx).placeholder("refine: words, -word, >1GB"));
        let subscriptions = vec![
            cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.search(window, cx);
                }
            }),
            cx.subscribe(&refine, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.observe(&engine, |_, _, cx| cx.notify()),
        ];
        Self {
            engine,
            query,
            refine,
            category: SearchCategory::All,
            status: Status::Idle,
            generation: 0,
            sort: Sort::Seeds,
            descending: true,
            rows: None,
            scroll: VirtualListScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.focus_handle(cx)
    }

    fn configured(&self, cx: &App) -> bool {
        self.engine
            .read(cx)
            .core()
            .settings()
            .indexers
            .iter()
            .any(|indexer| indexer.enabled)
    }

    fn search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().trim().to_string();
        if query.is_empty() {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.status = Status::Searching;
        let request = SearchRequest {
            query,
            category: self.category,
        };
        let job = self
            .engine
            .read(cx)
            .run(move |core| async move { core.search(request).await });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.status = match result {
                    Ok(reply) => Status::Done(reply),
                    Err(err) => Status::Failed(format!("{err:#}")),
                };
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn pick_category(
        &mut self,
        category: SearchCategory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.category = category;
        if !matches!(self.status, Status::Idle) {
            self.search(window, cx);
        }
        cx.notify();
    }

    fn sort_by(&mut self, sort: Sort, cx: &mut Context<Self>) {
        let natural = !matches!(sort, Sort::Name | Sort::Source | Sort::Age);
        if self.sort == sort {
            self.descending = !self.descending;
        } else {
            self.sort = sort;
            self.descending = natural;
        }
        cx.notify();
    }

    fn refresh(&mut self, cx: &App) -> Rc<Vec<SearchResult>> {
        let Status::Done(reply) = &self.status else {
            self.rows = None;
            return Rc::default();
        };
        let refine_text = self.refine.read(cx).value().to_string();
        let key = (
            self.generation,
            refine_text.clone(),
            self.sort,
            self.descending,
        );
        if let Some(rows) = &self.rows
            && rows.key == key
        {
            return rows.results.clone();
        }
        let refine = Refine::parse(&refine_text);
        let mut results: Vec<SearchResult> = reply
            .results
            .iter()
            .filter(|result| refine.matches(&result.title, result.size))
            .cloned()
            .collect();
        let (sort, descending) = (self.sort, self.descending);
        results.sort_by(|a, b| {
            let order = compare(sort, a, b);
            if descending { order.reverse() } else { order }
        });
        let results = Rc::new(results);
        let sizes = Rc::new(vec![size(px(1.), px(ROW)); results.len()]);
        self.rows = Some(Rows {
            key,
            results: results.clone(),
            sizes,
        });
        results
    }

    fn have(&self, cx: &App) -> HashSet<Id> {
        self.engine
            .read(cx)
            .snapshot
            .torrents
            .iter()
            .map(|torrent| torrent.id.clone())
            .collect()
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = palette(cx);
        let results = self.refresh(cx);
        let have = self.have(cx);
        let now = bawkbit_core::now();
        range
            .filter_map(|ix| results.get(ix).map(|result| (ix, result)))
            .map(|(ix, result)| row(ix, result, &have, now, &p, cx).into_any_element())
            .collect()
    }

    fn header(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let column = |sort: Sort, label: &'static str, width: Option<f32>| {
            let active = self.sort == sort;
            let arrow = if self.descending { "↓" } else { "↑" };
            div()
                .id(SharedString::from(format!("search-sort-{sort:?}")))
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
                .child(label)
                .when(active, |this| this.child(arrow))
                .on_click(cx.listener(move |this, _, _, cx| this.sort_by(sort, cx)))
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
            .child(column(Sort::Name, "name", None))
            .child(column(Sort::Size, "size", Some(76.)))
            .child(column(Sort::Seeds, "seeds", Some(56.)))
            .child(column(Sort::Peers, "peers", Some(56.)))
            .child(column(Sort::Age, "age", Some(48.)))
            .child(column(Sort::Source, "source", Some(150.)))
            .child(div().w(px(16.)).flex_none())
    }

    fn render_toolbar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let searching = matches!(self.status, Status::Searching);
        div()
            .px_3()
            .pt_3()
            .pb_2()
            .flex()
            .flex_col()
            .gap_2()
            .border_b_1()
            .border_color(p.border_weak)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div().flex_1().max_w(px(560.)).child(
                            kit::input(&self.query)
                                .prefix(
                                    Icon::new(IconName::Search).size(px(14.)).text_color(p.icon),
                                )
                                .cleanable(true),
                        ),
                    )
                    .when(searching, |this| {
                        this.child(Spinner::new().small().color(p.yolk))
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(SearchCategory::ALL.map(|category| {
                        kit::chip(
                            category.label(),
                            category.label(),
                            category == self.category,
                            p,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.pick_category(category, window, cx),
                        ))
                    }))
                    .child(div().flex_1())
                    .when(matches!(self.status, Status::Done(_)), |this| {
                        this.child(
                            div()
                                .w(px(240.))
                                .child(kit::input(&self.refine).small().cleanable(true)),
                        )
                    }),
            )
    }

    fn render_status(&self, shown: usize, p: &Palette) -> Option<Div> {
        let Status::Done(reply) = &self.status else {
            return None;
        };
        let total = reply.results.len();
        let count = if shown == total {
            format!("{} results", format::count(total))
        } else {
            format!(
                "{} of {} results",
                format::count(shown),
                format::count(total)
            )
        };
        let failures = reply
            .failures
            .iter()
            .map(|failure| format!("{}: {}", failure.source, failure.error))
            .collect::<Vec<_>>()
            .join(" · ");
        Some(
            div()
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .gap_3()
                .border_t_1()
                .border_color(p.border_weak)
                .text_size(px(11.))
                .text_color(p.text_weak)
                .child(div().flex_none().child(count))
                .when(!failures.is_empty(), |this| {
                    this.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .text_color(p.warning)
                            .child(kit::dot(p.warning))
                            .child(kit::truncate(failures)),
                    )
                }),
        )
    }
}

fn compare(sort: Sort, a: &SearchResult, b: &SearchResult) -> Ordering {
    match sort {
        Sort::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        Sort::Size => a.size.cmp(&b.size),
        Sort::Seeds => a.seeds.unwrap_or(0).cmp(&b.seeds.unwrap_or(0)),
        Sort::Peers => a.peers.unwrap_or(0).cmp(&b.peers.unwrap_or(0)),
        Sort::Age => b.published.unwrap_or(0).cmp(&a.published.unwrap_or(0)),
        Sort::Source => a.source.to_lowercase().cmp(&b.source.to_lowercase()),
    }
}

fn seed_color(seeds: Option<u32>, p: &Palette) -> Hsla {
    match seeds {
        Some(0) | None => p.text_weaker,
        Some(1..=9) => p.yellow,
        Some(_) => p.green,
    }
}

fn row(
    ix: usize,
    result: &SearchResult,
    have: &HashSet<Id>,
    now: i64,
    p: &Palette,
    cx: &mut Context<SearchView>,
) -> impl IntoElement {
    let owned = result
        .info_hash
        .as_ref()
        .filter(|hash| have.contains(*hash))
        .cloned();
    let click = (result.link.clone(), owned.clone());
    let menu_result = result.clone();
    let view = cx.entity().downgrade();
    let number = |value: Option<u32>| value.map(|n| format::count(n as usize)).unwrap_or_default();
    let tip: SharedString = format!("{} · via {}", result.source, result.via).into();
    div()
        .id(("result", ix))
        .w_full()
        .h(px(ROW))
        .px_3()
        .flex()
        .items_center()
        .gap_3()
        .border_b_1()
        .border_color(p.border_weak)
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|style| style.bg(p.bg_weak))
        .on_click(cx.listener(move |_, _, _, cx| match &click.1 {
            Some(id) => cx.emit(SearchEvent::Reveal(id.clone())),
            None => cx.emit(SearchEvent::Add(click.0.clone())),
        }))
        .context_menu(move |menu, window, cx| {
            result_menu(menu, &menu_result, view.clone(), window, cx)
        })
        .child(
            kit::truncate(result.title.clone())
                .flex_1()
                .text_size(px(13.))
                .text_color(p.text_strong),
        )
        .child(cell(format::bytes(result.size), 76., p.text_weak))
        .child(cell(number(result.seeds), 56., seed_color(result.seeds, p)))
        .child(cell(number(result.peers), 56., p.text_weak))
        .child(cell(format::age(result.published, now), 48., p.text_weak))
        .child(
            div()
                .id(("source", ix))
                .w(px(150.))
                .flex_none()
                .flex()
                .justify_end()
                .child(kit::truncate(result.source.clone()).text_color(p.text_weak))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx)),
        )
        .child(div().w(px(16.)).flex_none().when(owned.is_some(), |this| {
            this.child(
                Icon::new(IconName::CircleCheck)
                    .size(px(14.))
                    .text_color(p.green),
            )
        }))
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

fn result_menu(
    menu: PopupMenu,
    result: &SearchResult,
    view: WeakEntity<SearchView>,
    _: &mut Window,
    _: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (add_view, link) = (view.clone(), result.link.clone());
    let copy = result.link.clone();
    let menu = menu
        .min_w(px(180.))
        .item(
            PopupMenuItem::new("add")
                .icon(Icon::new(IconName::Plus))
                .on_click(move |_, _, cx| {
                    let _ = add_view.update(cx, |_, cx| cx.emit(SearchEvent::Add(link.clone())));
                }),
        )
        .item(
            PopupMenuItem::new("copy link")
                .icon(Icon::new(IconName::Copy))
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                    window.push_notification("link copied", cx);
                }),
        );
    match result.info_url.clone() {
        Some(url) => menu.item(
            PopupMenuItem::new("open page")
                .icon(Icon::new(IconName::ExternalLink))
                .on_click(move |_, _, cx| cx.open_url(&url)),
        ),
        None => menu,
    }
}

impl Render for SearchView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        if !self.configured(cx) {
            return div().size_full().child(kit::empty_state(
                "search asks your prowlarr or jackett. add one to start.",
                kit::button("configure-search", cx)
                    .label("set up search")
                    .icon(Icon::new(IconName::Settings))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SearchEvent::Configure))),
                &p,
            ));
        }
        let results = self.refresh(cx);
        let sizes = self
            .rows
            .as_ref()
            .map(|rows| rows.sizes.clone())
            .unwrap_or_default();
        let body = match &self.status {
            Status::Idle => placeholder("type a name and press enter", &p),
            Status::Searching => placeholder("asking your indexers…", &p),
            Status::Failed(error) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(p.danger)
                .child(kit::dot(p.danger))
                .child(error.clone())
                .into_any_element(),
            Status::Done(_) if results.is_empty() => placeholder("nothing found", &p),
            Status::Done(_) => div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    v_virtual_list(
                        cx.entity(),
                        "search-results",
                        sizes,
                        |this, range, _, cx| this.render_rows(range, cx),
                    )
                    .track_scroll(&self.scroll),
                )
                .vertical_scrollbar(&self.scroll)
                .into_any_element(),
        };
        let has_rows = matches!(self.status, Status::Done(_)) && !results.is_empty();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_toolbar(&p, cx))
            .when(has_rows, |this| this.child(self.header(&p, cx)))
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .children(self.render_status(results.len(), &p))
    }
}

fn placeholder(text: &'static str, p: &Palette) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_color(p.text_weaker)
        .child(text)
        .into_any_element()
}
