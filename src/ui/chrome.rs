use bawkbit_core::{Action, Snapshot};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable, TitleBar};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::filter::Status;
use super::{Page, Workspace, act, kit, prompts};
use crate::assets::MARK;
use crate::engine::Engine;
use crate::format;
use crate::theme::{Palette, palette};

const GRAPH_SAMPLES: usize = 60;

fn wordmark(p: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(7.))
        .child(svg().path(MARK).size(px(17.)).text_color(p.text_strong))
        .child(
            div()
                .flex()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(div().text_color(p.text_strong).child("bawk"))
                .child(div().text_color(p.text_weak).child("bit")),
        )
}

pub fn bare_title_bar(cx: &App) -> impl IntoElement {
    let p = palette(cx);
    TitleBar::new()
        .h(px(40.))
        .bg(p.bg)
        .border_color(p.border_weak)
        .child(wordmark(&p))
}

pub fn title_bar(
    snapshot: &Snapshot,
    engine: &Entity<Engine>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let p = palette(cx);
    let totals = &snapshot.totals;
    let rate = |icon: IconName, value: u64, limit: u64, color: Hsla| {
        let text = if value == 0 {
            "0 B/s".to_string()
        } else {
            format::speed(value)
        };
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .min_w(px(92.))
            .child(Icon::new(icon).size(px(13.)).text_color(color))
            .child(div().text_color(p.text_strong).child(text))
            .when(limit > 0, |this| {
                this.child(
                    div()
                        .text_color(p.text_weaker)
                        .child(format!("/ {}", format::bytes(limit))),
                )
            })
    };
    let alt = totals.alt_speed;
    let alt_tip: SharedString = if alt {
        "alternative speed limits on".into()
    } else {
        "alternative speed limits off".into()
    };
    let engine = engine.clone();
    let alt_button = Button::new("alt-speed")
        .ghost()
        .small()
        .icon(
            Icon::new(IconName::Turtle)
                .size(px(15.))
                .text_color(if alt { p.yolk } else { p.icon }),
        )
        .when(alt, |this| this.bg(p.yolk_dim))
        .tooltip(alt_tip)
        .on_click(cx.listener(move |_, _, window, cx| {
            act(&engine, Action::SetAltSpeed { on: !alt }, window, cx)
        }));
    let _ = window;

    let speeds = div()
        .flex()
        .items_center()
        .gap_3()
        .pr_3()
        .text_size(px(12.))
        .child(graph(&snapshot.history, &p))
        .child(rate(
            IconName::ArrowDown,
            totals.down,
            totals.down_limit,
            p.blue,
        ))
        .child(rate(IconName::ArrowUp, totals.up, totals.up_limit, p.green))
        .child(alt_button);

    TitleBar::new()
        .h(px(40.))
        .bg(p.bg)
        .border_color(p.border_weak)
        .child(wordmark(&p))
        .child(speeds)
}

/// The last minute of download (bars) and upload (a line on top) speed.
fn graph(history: &[[u64; 2]], p: &Palette) -> impl IntoElement {
    let samples: Vec<[u64; 2]> = history
        .iter()
        .skip(history.len().saturating_sub(GRAPH_SAMPLES))
        .copied()
        .collect();
    let peak = samples
        .iter()
        .map(|sample| sample[0].max(sample[1]))
        .max()
        .unwrap_or(0)
        .max(1);
    let (down, up, base) = (p.blue.opacity(0.55), p.green, p.border_weak);
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let step = bounds.size.width / GRAPH_SAMPLES as f32;
            let bottom = bounds.origin.y + bounds.size.height;
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.origin.x, bottom - px(1.)),
                    size(bounds.size.width, px(1.)),
                ),
                base,
            ));
            let offset = GRAPH_SAMPLES - samples.len();
            for (ix, sample) in samples.iter().enumerate() {
                let x = bounds.origin.x + step * (offset + ix) as f32;
                let height = bounds.size.height * (sample[0] as f32 / peak as f32);
                if height > px(0.) {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(x, bottom - height),
                            size((step - px(1.)).max(px(1.)), height),
                        ),
                        down,
                    ));
                }
                if sample[1] > 0 {
                    let y = bottom - bounds.size.height * (sample[1] as f32 / peak as f32);
                    window.paint_quad(fill(
                        Bounds::new(point(x, y.min(bottom - px(1.5))), size(step, px(1.5))),
                        up,
                    ));
                }
            }
        },
    )
    .w(px(96.))
    .h(px(20.))
}

impl Workspace {
    pub(super) fn render_sidebar(
        &self,
        snapshot: &Snapshot,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = palette(cx);
        let list = self.list.read(cx);
        let filters = list.filters.clone();
        let on_torrents = self.page == Page::Torrents;

        let item =
            |id: SharedString, icon: IconName, label: SharedString, count: usize, active: bool| {
                div()
                    .id(id)
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(if active { p.text_strong } else { p.text })
                    .when(active, |this| this.bg(p.bg_hover))
                    .when(!active, |this| this.hover(|style| style.bg(p.bg_weak)))
                    .child(Icon::new(icon).small().text_color(if active {
                        p.text_strong
                    } else {
                        p.icon
                    }))
                    .child(kit::truncate(label).flex_1())
                    .when(count > 0, |this| {
                        this.child(
                            div()
                                .text_size(px(12.))
                                .text_color(p.text_weak)
                                .child(format::count(count)),
                        )
                    })
            };
        let divider = || div().my_2().mx(px(10.)).h(px(1.)).bg(p.border_weak);

        let statuses = Status::ALL.into_iter().filter_map(|status| {
            let count = snapshot
                .torrents
                .iter()
                .filter(|t| status.matches(t))
                .count();
            if status == Status::Error && count == 0 {
                return None;
            }
            let active = on_torrents && filters.status == status;
            Some(
                item(
                    status.label().into(),
                    status.icon(),
                    status.label().into(),
                    count,
                    active,
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.set_status(status, window, cx)),
                ),
            )
        });
        let statuses: Vec<_> = statuses.collect();

        let categories: Vec<_> = snapshot
            .categories
            .iter()
            .map(|category| {
                let name = category.name.clone();
                let count = snapshot
                    .torrents
                    .iter()
                    .filter(|t| t.category.as_ref() == Some(&name))
                    .count();
                let active = on_torrents && filters.category.as_ref() == Some(&name);
                let (click, menu_name) = (name.clone(), name.clone());
                let limits = (category.down_limit, category.up_limit);
                let engine = self.engine.clone();
                item(
                    format!("category-{name}").into(),
                    IconName::Folder,
                    name.into(),
                    count,
                    active,
                )
                .when(limits != (0, 0), |this| {
                    this.child(Icon::new(IconName::Gauge).xsmall().text_color(p.icon))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.toggle_category(click.clone(), window, cx)
                }))
                .context_menu(move |menu, _, _| {
                    let name = menu_name.clone();
                    let (rename_engine, rename_name) = (engine.clone(), name.clone());
                    let (limit_engine, limit_name) = (engine.clone(), name.clone());
                    let (remove_engine, remove_name) = (engine.clone(), name.clone());
                    menu.item(
                        PopupMenuItem::new("rename")
                            .icon(Icon::new(IconName::Pencil))
                            .on_click(move |_, window, cx| {
                                let from = rename_name.clone();
                                let action = move |to| Action::RenameCategory {
                                    from: from.clone(),
                                    to,
                                };
                                let current = rename_name.clone();
                                prompts::rename(rename_engine.clone(), current, action, window, cx)
                            }),
                    )
                    .item(
                        PopupMenuItem::new("speed limits")
                            .icon(Icon::new(IconName::Gauge))
                            .on_click(move |_, window, cx| {
                                let name = limit_name.clone();
                                let action = move |down, up| Action::SetCategoryLimits {
                                    name: name.clone(),
                                    down,
                                    up,
                                };
                                let engine = limit_engine.clone();
                                prompts::speed_limits(engine, Some(limits), action, window, cx)
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("remove category")
                            .icon(Icon::new(IconName::Trash))
                            .on_click(move |_, window, cx| {
                                let name = remove_name.clone();
                                let title = format!("remove the category {name}?");
                                let body = match count {
                                    0 => "no torrent is in it.".to_string(),
                                    1 => "its torrent stays, with no category.".to_string(),
                                    n => format!("its {n} torrents stay, with no category."),
                                };
                                let action = Action::RemoveCategory { name };
                                let engine = remove_engine.clone();
                                prompts::confirm_action(engine, title, body, action, window, cx);
                            }),
                    )
                })
            })
            .collect();

        let tags: Vec<_> = snapshot
            .tags
            .iter()
            .map(|tag| {
                let tag = tag.clone();
                let count = snapshot
                    .torrents
                    .iter()
                    .filter(|t| t.tags.contains(&tag))
                    .count();
                let active = on_torrents && filters.tag.as_ref() == Some(&tag);
                let (click, remove) = (tag.clone(), tag.clone());
                let engine = self.engine.clone();
                item(
                    format!("tag-{tag}").into(),
                    IconName::Tag,
                    tag.into(),
                    count,
                    active,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.toggle_tag(click.clone(), window, cx)
                }))
                .context_menu(move |menu, _, _| {
                    let (engine, tag) = (engine.clone(), remove.clone());
                    let (rename_engine, rename_tag) = (engine.clone(), tag.clone());
                    menu.item(
                        PopupMenuItem::new("rename")
                            .icon(Icon::new(IconName::Pencil))
                            .on_click(move |_, window, cx| {
                                let from = rename_tag.clone();
                                let action = move |to| Action::RenameTag {
                                    from: from.clone(),
                                    to,
                                };
                                let current = rename_tag.clone();
                                prompts::rename(rename_engine.clone(), current, action, window, cx)
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("remove tag")
                            .icon(Icon::new(IconName::Trash))
                            .on_click(move |_, window, cx| {
                                let title = format!("remove the tag {tag}?");
                                let body = match count {
                                    0 => "no torrent has it.".to_string(),
                                    1 => "one torrent loses it.".to_string(),
                                    n => format!("{n} torrents lose it."),
                                };
                                let action = Action::RemoveTag { tag: tag.clone() };
                                prompts::confirm_action(
                                    engine.clone(),
                                    title,
                                    body,
                                    action,
                                    window,
                                    cx,
                                );
                            }),
                    )
                })
            })
            .collect();

        let search_active = self.page == Page::Search;
        let rss_active = self.page == Page::Rss;
        let settings_active = self.page == Page::Settings;
        let update = self.updates.read(cx).has_news();
        let has_categories = !categories.is_empty();
        let has_tags = !tags.is_empty();

        div()
            .w(px(200.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .justify_between()
            .p(px(8.))
            .border_r_1()
            .border_color(p.border_weak)
            .child(
                div()
                    .id("filters")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        item(
                            "search".into(),
                            IconName::Search,
                            "search".into(),
                            0,
                            search_active,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.select(Page::Search, window, cx)
                            }),
                        ),
                    )
                    .child(
                        item("rss".into(), IconName::Rss, "rss".into(), 0, rss_active).on_click(
                            cx.listener(|this, _, window, cx| this.select(Page::Rss, window, cx)),
                        ),
                    )
                    .child(divider())
                    .children(statuses)
                    .when(has_categories, |this| {
                        this.child(divider()).children(categories)
                    })
                    .when(has_tags, |this| this.child(divider()).children(tags)),
            )
            .child(
                item(
                    "settings".into(),
                    IconName::Settings,
                    "settings".into(),
                    0,
                    settings_active,
                )
                .when(update, |this| this.child(kit::dot(p.yolk)))
                .on_click(
                    cx.listener(|this, _, window, cx| this.select(Page::Settings, window, cx)),
                ),
            )
    }
}

/// Runs an action from a place with no view of its own, such as a menu item.
pub(super) fn run_action(
    engine: &Entity<Engine>,
    action: Action,
    window: &mut Window,
    cx: &mut App,
) {
    let job = engine
        .read(cx)
        .run(move |core| async move { core.execute(action).await.map(|_| ()) });
    window
        .spawn(cx, async move |cx| {
            if let Err(err) = job.await {
                let _ = cx.update(|window, cx| {
                    use gpui_kit::component::WindowExt as _;
                    window.push_notification(
                        gpui_kit::component::notification::Notification::error(format!("{err:#}")),
                        cx,
                    )
                });
            }
        })
        .detach();
}
