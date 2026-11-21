use std::rc::Rc;
use std::time::Duration;

use bawkbit_core::{Action, Details, DetailsPart, Id, Torrent, TrackerReport, TrackerStatus};
use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    Disableable, Icon, Sizable, VirtualListScrollHandle, WindowExt, v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::chrome::run_action;
use super::filter::state_label;
use super::{act, kit, prompts};
use crate::engine::Engine;
use crate::format;
use crate::theme::{Palette, palette};

const FILE_ROW: f32 = 30.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Files,
    Peers,
    Trackers,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Overview, Tab::Files, Tab::Peers, Tab::Trackers];

    fn label(self) -> &'static str {
        match self {
            Tab::Overview => "overview",
            Tab::Files => "files",
            Tab::Peers => "peers",
            Tab::Trackers => "trackers",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Tab::Overview => IconName::Info,
            Tab::Files => IconName::File,
            Tab::Peers => IconName::Users,
            Tab::Trackers => IconName::Server,
        }
    }

    fn part(self) -> DetailsPart {
        match self {
            Tab::Overview => DetailsPart::Overview,
            Tab::Files => DetailsPart::Files,
            Tab::Peers => DetailsPart::Peers,
            Tab::Trackers => DetailsPart::Trackers,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrackerSort {
    Address,
    WorkingFirst,
    FailingFirst,
}

impl TrackerSort {
    const ALL: [TrackerSort; 3] = [
        TrackerSort::Address,
        TrackerSort::WorkingFirst,
        TrackerSort::FailingFirst,
    ];

    fn label(self) -> &'static str {
        match self {
            TrackerSort::Address => "a–z",
            TrackerSort::WorkingFirst => "working first",
            TrackerSort::FailingFirst => "failing first",
        }
    }
}

fn status_rank(status: TrackerStatus) -> u8 {
    match status {
        TrackerStatus::Working => 0,
        TrackerStatus::Waiting => 1,
        TrackerStatus::Failing => 2,
    }
}

pub enum DetailsEvent {
    Close,
}

pub struct DetailsView {
    engine: Entity<Engine>,
    id: Option<Id>,
    details: Option<Details>,
    /// The tab `details` was fetched for; it holds only that tab's lists.
    part: Option<DetailsPart>,
    tab: Tab,
    tracker_sort: TrackerSort,
    files_scroll: VirtualListScrollHandle,
    file_sizes: Rc<Vec<Size<Pixels>>>,
    fetching: bool,
    _fetch: Task<()>,
    _poll: Task<()>,
}

impl EventEmitter<DetailsEvent> for DetailsView {}

impl DetailsView {
    pub fn new(engine: Entity<Engine>, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let polled = this.update(cx, |this, cx| {
                    if !this.fetching {
                        this.fetch(cx);
                    }
                });
                if polled.is_err() {
                    break;
                }
            }
        });
        Self {
            engine,
            id: None,
            details: None,
            part: None,
            tab: Tab::Overview,
            tracker_sort: TrackerSort::Address,
            files_scroll: VirtualListScrollHandle::new(),
            file_sizes: Rc::default(),
            fetching: false,
            _fetch: Task::ready(()),
            _poll: poll,
        }
    }

    pub fn is_showing(&self) -> bool {
        self.id.is_some()
    }

    pub fn show(&mut self, id: Option<Id>, cx: &mut Context<Self>) {
        if self.id == id {
            return;
        }
        self.id = id;
        self.details = None;
        self.part = None;
        self.fetch(cx);
    }

    /// Asks the engine, on its own runtime, for what the open tab shows; a newer fetch replaces one still running.
    fn fetch(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.id.clone() else {
            return;
        };
        let part = self.tab.part();
        let job = {
            let id = id.clone();
            self.engine
                .read(cx)
                .run(move |core| async move { core.details_part(&id, part) })
        };
        self.fetching = true;
        self._fetch = cx.spawn(async move |this, cx| {
            let details = job.await.ok();
            let _ = this.update(cx, |this, cx| {
                this.fetching = false;
                if this.id.as_ref() != Some(&id) || this.tab.part() != part {
                    return;
                }
                let files = details.as_ref().map_or(0, |d| d.files.len());
                if part == DetailsPart::Files && this.file_sizes.len() != files {
                    this.file_sizes = Rc::new(vec![size(px(1.), px(FILE_ROW)); files]);
                }
                this.details = details;
                this.part = Some(part);
                cx.notify();
            });
        });
        cx.notify();
    }

    fn torrent(&self, cx: &App) -> Option<Torrent> {
        let id = self.id.as_ref()?;
        self.engine
            .read(cx)
            .snapshot
            .torrents
            .iter()
            .find(|t| &t.id == id)
            .cloned()
    }

    fn toggle_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(details) = self.details.as_mut() else {
            return;
        };
        let mut files: Vec<usize> = details
            .files
            .iter()
            .filter(|file| file.included != (file.index == index))
            .map(|file| file.index)
            .collect();
        files.sort_unstable();
        if files.is_empty() {
            window.push_notification(Notification::warning("keep at least one file"), cx);
            return;
        }
        for file in &mut details.files {
            if file.index == index {
                file.included = !file.included;
            }
        }
        let action = Action::SetFiles {
            id: details.id.clone(),
            files,
        };
        act(&self.engine, action, window, cx);
        cx.notify();
    }

    fn render_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(36.))
            .px_3()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(p.border_weak)
            .children(Tab::ALL.map(|tab| {
                let active = tab == self.tab;
                div()
                    .id(tab.label())
                    .h(px(26.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(if active { p.text_strong } else { p.text_weak })
                    .when(active, |this| this.bg(p.bg_hover))
                    .hover(|style| style.text_color(p.text_strong))
                    .child(Icon::new(tab.icon()).size(px(13.)))
                    .child(tab.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.tab != tab {
                            this.tab = tab;
                            this.fetch(cx);
                        }
                    }))
            }))
            .child(div().flex_1())
            .child(
                kit::icon_button("close-details", IconName::X, "close")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(DetailsEvent::Close))),
            )
    }

    fn render_overview(
        &self,
        torrent: Option<&Torrent>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (Some(t), details) = (torrent, self.details.as_ref()) else {
            return div().into_any_element();
        };
        let color = p.state(t.state);
        let map = details.map(|d| d.piece_map.clone()).unwrap_or_default();
        let empty = p.border_weak;
        let pieces = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                window.paint_quad(fill(bounds, empty));
                if map.is_empty() {
                    return;
                }
                let step = bounds.size.width / map.len() as f32;
                let mut ix = 0;
                while ix < map.len() {
                    let value = map[ix];
                    let mut end = ix + 1;
                    while end < map.len() && map[end] == value {
                        end += 1;
                    }
                    if value > 0 {
                        let x = bounds.origin.x + step * ix as f32;
                        let width = step * (end - ix) as f32;
                        let shade = color.opacity(0.25 + 0.75 * value as f32 / 255.);
                        window.paint_quad(fill(
                            Bounds::new(point(x, bounds.origin.y), size(width, bounds.size.height)),
                            shade,
                        ));
                    }
                    ix = end;
                }
            },
        )
        .size_full();
        let pieces = div()
            .w_full()
            .h(px(12.))
            .flex_none()
            .rounded(px(2.))
            .overflow_hidden()
            .child(pieces);

        let done = format!(
            "{} of {} · {}",
            format::bytes(t.done),
            format::bytes(t.size),
            format::percent(t.progress)
        );
        let speed = match (t.down, t.up) {
            (0, 0) => "idle".to_string(),
            (down, up) => format!(
                "↓ {}  ↑ {}",
                format::bytes(down) + "/s",
                format::bytes(up) + "/s"
            ),
        };
        let pieces_text = details
            .map(|d| {
                format!(
                    "{} × {}",
                    format::count(d.pieces as usize),
                    format::bytes(d.piece_size as u64)
                )
            })
            .unwrap_or_default();
        let state = match &t.error {
            Some(error) => format!("{}: {error}", state_label(t.state)),
            None => state_label(t.state).to_string(),
        };
        let path = t.save_path.clone();
        let magnet = details.map(|d| d.magnet.clone());

        div()
            .id("overview")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(pieces)
            .child(
                div()
                    .grid()
                    .grid_cols(4)
                    .gap_x_6()
                    .gap_y_3()
                    .child(kit::stat("state", state, p))
                    .child(kit::stat("downloaded", done, p))
                    .child(kit::stat("speed", speed, p))
                    .child(kit::stat("time left", nonblank(format::eta(t.eta)), p))
                    .child(kit::stat("uploaded", format::bytes(t.uploaded), p))
                    .child(kit::stat("ratio", format::ratio(t.ratio), p))
                    .child(kit::stat(
                        "seeds / peers",
                        format!("{} / {}", t.seeds, t.peers),
                        p,
                    ))
                    .child(kit::stat("pieces", nonblank(pieces_text), p))
                    .child(kit::stat("added", format::date(t.added), p))
                    .child(kit::stat(
                        "completed",
                        nonblank(t.completed.map(format::date).unwrap_or_default()),
                        p,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        kit::button("open-folder", cx)
                            .icon(Icon::new(IconName::FolderOpen))
                            .label(SharedString::from(path.clone()))
                            .on_click(move |_, _, cx| {
                                cx.open_with_system(std::path::Path::new(&path))
                            }),
                    )
                    .when_some(magnet, |this, magnet| {
                        this.child(
                            kit::button("copy-magnet", cx)
                                .icon(Icon::new(IconName::Magnet))
                                .tooltip("copy magnet link")
                                .on_click(move |_, window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        magnet.clone(),
                                    ));
                                    window.push_notification("magnet link copied", cx);
                                }),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_files(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        if self.details.as_ref().is_none_or(|d| d.files.is_empty()) {
            return placeholder("no file list yet", p);
        }
        div()
            .size_full()
            .relative()
            .child(
                v_virtual_list(
                    cx.entity(),
                    "files",
                    self.file_sizes.clone(),
                    |this, range, window, cx| {
                        let p = palette(cx);
                        let Some(details) = this.details.as_ref() else {
                            return Vec::new();
                        };
                        let single = details.files.len() == 1;
                        range
                            .filter_map(|ix| details.files.get(ix).cloned())
                            .map(|file| {
                                let (folder, name) = match file.path.rsplit_once('/') {
                                    Some((folder, name)) => {
                                        (format!("{folder}/"), name.to_string())
                                    }
                                    None => (String::new(), file.path.clone()),
                                };
                                let progress = file.done as f32 / file.size.max(1) as f32;
                                let index = file.index;
                                let _ = window;
                                div()
                                    .h(px(FILE_ROW))
                                    .px_4()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .text_size(px(12.))
                                    .border_b_1()
                                    .border_color(p.border_weak)
                                    .child(
                                        Checkbox::new(("file", index))
                                            .checked(file.included)
                                            .when(single, |this| this.disabled(true))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.toggle_file(index, window, cx)
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .max_w(relative(0.5))
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .text_color(p.text_weaker)
                                                    .child(folder),
                                            )
                                            .child(kit::truncate(name).text_color(
                                                if file.included {
                                                    p.text_strong
                                                } else {
                                                    p.text_weak
                                                },
                                            )),
                                    )
                                    .child(
                                        div()
                                            .w(px(72.))
                                            .flex_none()
                                            .flex()
                                            .justify_end()
                                            .text_color(p.text_weak)
                                            .child(format::bytes(file.size)),
                                    )
                                    .child(
                                        div()
                                            .w(px(96.))
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(kit::progress_bar(
                                                progress,
                                                if progress >= 1. { p.green } else { p.blue },
                                                &p,
                                            ))
                                            .child(
                                                div()
                                                    .w(px(40.))
                                                    .flex_none()
                                                    .flex()
                                                    .justify_end()
                                                    .text_color(p.text_weak)
                                                    .child(format::percent(progress)),
                                            ),
                                    )
                                    .into_any_element()
                            })
                            .collect()
                    },
                )
                .track_scroll(&self.files_scroll),
            )
            .vertical_scrollbar(&self.files_scroll)
            .into_any_element()
    }

    fn render_peers(&self, p: &Palette) -> AnyElement {
        let peers = self
            .details
            .as_ref()
            .map(|d| d.peers.clone())
            .unwrap_or_default();
        if peers.is_empty() {
            return placeholder("no peers connected", p);
        }
        let countries = peers.iter().any(|peer| peer.country.is_some());
        div()
            .id("peers")
            .size_full()
            .overflow_y_scroll()
            .text_size(px(12.))
            .children(peers.into_iter().enumerate().map(|(ix, peer)| {
                let country = peer.country.clone().unwrap_or_default();
                let flag = crate::assets::flag(&country);
                div()
                    .h(px(28.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(p.border_weak)
                    .when(countries, |row| {
                        row.child(
                            div()
                                .w(px(44.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_color(p.text_weak)
                                .child(match flag {
                                    Some(path) => img(path)
                                        .w(px(18.))
                                        .h(px(12.))
                                        .flex_none()
                                        .rounded(px(2.))
                                        .into_any_element(),
                                    None => div().w(px(18.)).flex_none().into_any_element(),
                                })
                                .child(country),
                        )
                    })
                    .child(
                        kit::truncate(peer.addr)
                            .w(px(220.))
                            .flex_none()
                            .text_color(p.text_strong),
                    )
                    .child(
                        kit::truncate(peer.client.unwrap_or_default())
                            .flex_1()
                            .text_color(p.text),
                    )
                    .child(
                        div()
                            .id(("peer-has", ix))
                            .w(px(96.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(kit::progress_bar(
                                peer.progress,
                                if peer.progress >= 1. { p.green } else { p.blue },
                                p,
                            ))
                            .child(
                                div()
                                    .w(px(40.))
                                    .flex_none()
                                    .flex()
                                    .justify_end()
                                    .text_color(p.text_weak)
                                    .child(format::percent(peer.progress)),
                            )
                            .tooltip(|window, cx| {
                                Tooltip::new("how much of the torrent this peer has")
                                    .build(window, cx)
                            }),
                    )
                    .child(
                        div()
                            .w(px(40.))
                            .flex_none()
                            .text_color(p.text_weaker)
                            .child(peer.transport.unwrap_or_default()),
                    )
                    .child(
                        div()
                            .w(px(88.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_color(p.blue)
                            .child(format::speed(peer.down)),
                    )
                    .child(
                        div()
                            .w(px(88.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_color(p.green)
                            .child(format::speed(peer.up)),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_color(p.text_weak)
                            .child(format::bytes(peer.downloaded)),
                    )
            }))
            .into_any_element()
    }

    fn render_trackers(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(details) = self.details.as_ref() else {
            return placeholder("no trackers: peers come from DHT", p);
        };
        let (engine, id, trackers) = (
            self.engine.clone(),
            details.id.clone(),
            details.trackers.clone(),
        );
        let mut rows: Vec<(String, Option<TrackerReport>)> = trackers
            .iter()
            .map(|url| {
                let report = details.tracker_reports.iter().find(|r| &r.url == url);
                (url.clone(), report.cloned())
            })
            .collect();
        let rank = |report: &Option<TrackerReport>| {
            status_rank(report.as_ref().map_or(TrackerStatus::Waiting, |r| r.status))
        };
        match self.tracker_sort {
            TrackerSort::Address => {}
            TrackerSort::WorkingFirst => rows.sort_by_key(|(_, report)| rank(report)),
            TrackerSort::FailingFirst => {
                rows.sort_by_key(|(_, report)| std::cmp::Reverse(rank(report)))
            }
        }
        let failing: Vec<String> = rows
            .iter()
            .filter(|(_, report)| {
                report
                    .as_ref()
                    .is_some_and(|r| r.status == TrackerStatus::Failing)
            })
            .map(|(url, _)| url.clone())
            .collect();
        let list: AnyElement = if trackers.is_empty() {
            placeholder("no trackers: peers come from DHT", p)
        } else {
            div()
                .id("trackers")
                .size_full()
                .overflow_y_scroll()
                .py_2()
                .text_size(px(12.))
                .children(rows.into_iter().enumerate().map(|(ix, (url, report))| {
                    let rest: Vec<String> =
                        trackers.iter().filter(|t| **t != url).cloned().collect();
                    let (engine, id) = (engine.clone(), id.clone());
                    let (color, label, detail) = tracker_state(report.as_ref(), p);
                    div()
                        .h(px(28.))
                        .pl_4()
                        .pr_2()
                        .flex()
                        .items_center()
                        .gap_3()
                        .text_color(p.text)
                        .child(kit::dot(color))
                        .child(kit::truncate(url.clone()).flex_1())
                        .child(div().w(px(56.)).flex_none().text_color(color).child(label))
                        .child(
                            div()
                                .id(("tracker-detail", ix))
                                .w(px(240.))
                                .flex_none()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_color(p.text_weak)
                                .child(detail.clone())
                                .tooltip(move |window, cx| {
                                    Tooltip::new(detail.clone()).build(window, cx)
                                }),
                        )
                        .child(
                            kit::button(("remove-tracker", ix), cx)
                                .xsmall()
                                .icon(Icon::new(IconName::X))
                                .tooltip("remove")
                                .on_click(move |_, window, cx| {
                                    let action = Action::SetTrackers {
                                        id: id.clone(),
                                        trackers: rest.clone(),
                                    };
                                    run_action(&engine, action, window, cx);
                                }),
                        )
                }))
                .into_any_element()
        };
        let private = details.private;
        let (list_engine, list_id) = (engine.clone(), id.clone());
        let (failing_engine, failing_id) = (engine.clone(), id.clone());
        let failing_count = failing.len();
        let kept: Vec<String> = trackers
            .iter()
            .filter(|url| !failing.contains(url))
            .cloned()
            .collect();
        let sort = kit::segmented(p)
            .text_size(px(12.))
            .children(TrackerSort::ALL.map(|sort| {
                kit::segment(
                    SharedString::from(format!("tracker-sort-{}", sort.label())),
                    sort.label(),
                    sort == self.tracker_sort,
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tracker_sort = sort;
                    cx.notify();
                }))
            }));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1().min_h_0().child(list))
            .child(
                div()
                    .px_4()
                    .py_2()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        kit::button("add-trackers", cx)
                            .small()
                            .icon(Icon::new(IconName::ListPlus))
                            .label("add trackers")
                            .on_click(move |_, window, cx| {
                                prompts::add_trackers(engine.clone(), id.clone(), window, cx)
                            }),
                    )
                    .when(!private, |this| {
                        this.child(
                            kit::button("add-list-trackers", cx)
                                .small()
                                .icon(Icon::new(IconName::Globe))
                                .label("add public trackers")
                                .tooltip("from the lists under settings, trackers")
                                .on_click(move |_, window, cx| {
                                    let action = Action::AddListTrackers {
                                        ids: vec![list_id.clone()],
                                    };
                                    run_action(&list_engine, action, window, cx);
                                }),
                        )
                    })
                    .when(failing_count > 0, |this| {
                        this.child(
                            kit::danger_button("remove-failing-trackers", cx)
                                .small()
                                .icon(Icon::new(IconName::Trash))
                                .label(format!("remove failing ({failing_count})"))
                                .on_click(move |_, window, cx| {
                                    let body = if kept.is_empty() {
                                        remove_all_failing_body(failing_count, private)
                                    } else {
                                        format!(
                                            "take {} off this torrent? the other {} stay.",
                                            trackers_word(failing_count),
                                            kept.len()
                                        )
                                    };
                                    let action = Action::SetTrackers {
                                        id: failing_id.clone(),
                                        trackers: kept.clone(),
                                    };
                                    prompts::confirm_action(
                                        failing_engine.clone(),
                                        "remove failing trackers",
                                        body,
                                        action,
                                        window,
                                        cx,
                                    );
                                }),
                        )
                    })
                    .when(private, |this| {
                        this.child(
                            div()
                                .text_size(px(12.))
                                .text_color(p.text_weaker)
                                .child("private torrent: public trackers stay off it."),
                        )
                    })
                    .child(div().flex_1())
                    .when(trackers.len() > 1, |this| this.child(sort)),
            )
            .into_any_element()
    }
}

/// The dot color, the one-word state and the rest of what a tracker last said.
fn tracker_state(
    report: Option<&TrackerReport>,
    p: &Palette,
) -> (Hsla, &'static str, SharedString) {
    let Some(report) = report else {
        return (p.text_weaker, "waiting", "not asked yet".into());
    };
    let when = report
        .updated
        .map(|at| format!(" · {} ago", format::age(Some(at), bawkbit_core::now())))
        .unwrap_or_default();
    match report.status {
        TrackerStatus::Waiting => (p.text_weaker, "waiting", "not asked yet".into()),
        TrackerStatus::Working => {
            let swarm = match (report.seeds, report.leechers) {
                (Some(seeds), Some(leechers)) => format!("{seeds} seeds · {leechers} leechers"),
                _ => format!("{} peers", report.peers),
            };
            (p.green, "working", format!("{swarm}{when}").into())
        }
        TrackerStatus::Failing => {
            let message = report.message.clone().unwrap_or_else(|| "no answer".into());
            (p.danger, "failing", format!("{message}{when}").into())
        }
    }
}

fn trackers_word(n: usize) -> String {
    if n == 1 {
        "1 failing tracker".into()
    } else {
        format!("{n} failing trackers")
    }
}

fn remove_all_failing_body(n: usize, private: bool) -> String {
    let after = if private {
        "a private torrent finds peers only through its trackers, so it will find none."
    } else {
        "peers will come from DHT only."
    };
    format!("every tracker is failing. take all {n} off? {after}")
}

fn nonblank(text: String) -> String {
    if text.is_empty() { "–".into() } else { text }
}

fn placeholder(text: &'static str, p: &Palette) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(p.text_weaker)
        .child(text)
        .into_any_element()
}

impl Render for DetailsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let torrent = self.torrent(cx);
        let body = match self.tab {
            _ if self.part != Some(self.tab.part()) => div().into_any_element(),
            Tab::Overview => self.render_overview(torrent.as_ref(), &p, cx),
            Tab::Files => self.render_files(&p, cx),
            Tab::Peers => self.render_peers(&p),
            Tab::Trackers => self.render_trackers(&p, cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(p.border_weak)
            .bg(p.bg)
            .child(self.render_tabs(&p, cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}
