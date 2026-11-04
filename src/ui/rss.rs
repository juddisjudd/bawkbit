use bawkbit_core::{Feed, FeedItem, Rss, RssAction, Rule, RuleMatch};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{kit, prompts};
use crate::engine::Engine;
use crate::format;
use crate::theme::{Palette, palette};

const ROW: f32 = 36.;

pub enum RssEvent {
    Add(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Feeds,
    Rules,
}

struct Editor {
    id: String,
    name: Entity<InputState>,
    must: Entity<InputState>,
    must_not: Entity<InputState>,
    category: Entity<InputState>,
    save_path: Entity<InputState>,
    enabled: bool,
    regex: bool,
    smart: bool,
    paused: bool,
    feeds: Vec<String>,
    preview: Result<Vec<RuleMatch>, String>,
    _subscriptions: Vec<Subscription>,
}

pub struct RssView {
    engine: Entity<Engine>,
    rss: Rss,
    rev: u64,
    tab: Tab,
    feed: Option<String>,
    url: Entity<InputState>,
    editor: Option<Editor>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RssEvent> for RssView {}

impl RssView {
    pub fn new(engine: Entity<Engine>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("feed address"));
        let subscriptions = vec![
            cx.subscribe_in(&url, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add_feed(window, cx);
                }
            }),
            cx.observe(&engine, |this, engine, cx| {
                let rev = engine.read(cx).snapshot.rss_rev;
                if rev != this.rev {
                    this.rev = rev;
                    this.rss = engine.read(cx).core().rss();
                    this.refresh_preview(cx);
                    cx.notify();
                }
            }),
        ];
        let rss = engine.read(cx).core().rss();
        let rev = engine.read(cx).snapshot.rss_rev;
        let feed = rss.feeds.first().map(|feed| feed.id.clone());
        Self {
            engine,
            rss,
            rev,
            tab: Tab::Feeds,
            feed,
            url,
            editor: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.url.focus_handle(cx)
    }

    fn run(
        &mut self,
        action: RssAction,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let job = self
            .engine
            .read(cx)
            .run(move |core| async move { core.rss_execute(action).await });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(rss) => {
                    this.rss = rss;
                    then(this, window, cx);
                    this.refresh_preview(cx);
                    cx.notify();
                }
                Err(err) => {
                    window.push_notification(Notification::error(format!("{err:#}")), cx);
                }
            });
        })
        .detach();
    }

    fn add_feed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.url.read(cx).value().trim().to_string();
        if url.is_empty() {
            return;
        }
        let then = |this: &mut Self, _: &mut Window, _: &mut Context<Self>| {
            this.feed = this.rss.feeds.last().map(|feed| feed.id.clone());
        };
        self.url
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.run(RssAction::AddFeed { url }, then, window, cx);
    }

    fn selected_feed(&self) -> Option<&Feed> {
        let picked = self.feed.as_ref();
        let feeds = &self.rss.feeds;
        feeds
            .iter()
            .find(|feed| Some(&feed.id) == picked)
            .or_else(|| feeds.first())
    }

    fn edit(&mut self, rule: Rule, window: &mut Window, cx: &mut Context<Self>) {
        let mut text = |value: &str, placeholder: &'static str, cx: &mut Context<Self>| {
            let value = value.to_string();
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let name = text(&rule.name, "name", cx);
        let must = text(&rule.must_contain, "show 1080p | other show", cx);
        let must_not = text(&rule.must_not_contain, "cam", cx);
        let category = text(rule.category.as_deref().unwrap_or(""), "none", cx);
        let save_path = text(
            rule.save_path.as_deref().unwrap_or(""),
            "the category's folder",
            cx,
        );
        let subscriptions = [&must, &must_not]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.refresh_preview(cx);
                        cx.notify();
                    }
                })
            })
            .collect();
        self.editor = Some(Editor {
            id: rule.id,
            name,
            must,
            must_not,
            category,
            save_path,
            enabled: rule.enabled,
            regex: rule.regex,
            smart: rule.smart_episodes,
            paused: rule.paused,
            feeds: rule.feeds,
            preview: Ok(Vec::new()),
            _subscriptions: subscriptions,
        });
        self.refresh_preview(cx);
        cx.notify();
    }

    fn editor_rule(&self, cx: &App) -> Option<Rule> {
        let editor = self.editor.as_ref()?;
        let value = |input: &Entity<InputState>| input.read(cx).value().trim().to_string();
        let optional = |input: &Entity<InputState>| Some(value(input)).filter(|v| !v.is_empty());
        Some(Rule {
            id: editor.id.clone(),
            name: value(&editor.name),
            enabled: editor.enabled,
            must_contain: value(&editor.must),
            must_not_contain: value(&editor.must_not),
            regex: editor.regex,
            smart_episodes: editor.smart,
            feeds: editor.feeds.clone(),
            category: optional(&editor.category),
            save_path: optional(&editor.save_path),
            paused: editor.paused,
            ..Rule::default()
        })
    }

    fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        let Some(rule) = self.editor_rule(cx) else {
            return;
        };
        let preview = self
            .engine
            .read(cx)
            .core()
            .rule_matches(&rule)
            .map_err(|err| format!("{err:#}"));
        if let Some(editor) = &mut self.editor {
            editor.preview = preview;
        }
    }

    fn save_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rule) = self.editor_rule(cx) else {
            return;
        };
        let added = rule.id.is_empty();
        let then = move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            if added && let (Some(editor), Some(saved)) = (&mut this.editor, this.rss.rules.last())
            {
                editor.id = saved.id.clone();
            }
            window.push_notification(Notification::success("rule saved"), cx);
        };
        self.run(RssAction::SaveRule { rule }, then, window, cx);
    }

    fn remove_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.take() else {
            return;
        };
        if !editor.id.is_empty() {
            self.run(
                RssAction::RemoveRule { id: editor.id },
                |_, _, _| {},
                window,
                cx,
            );
        }
        cx.notify();
    }

    fn render_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |tab: Tab, label: &'static str| {
            kit::segment(label, label, self.tab == tab, p).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.tab = tab;
                    cx.notify();
                },
            ))
        };
        div()
            .px_3()
            .pt_3()
            .pb_2()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(p.border_weak)
            .child(
                kit::segmented(p)
                    .child(tab(Tab::Feeds, "feeds"))
                    .child(tab(Tab::Rules, "rules")),
            )
            .child(div().flex_1())
            .when(
                self.tab == Tab::Feeds && !self.rss.feeds.is_empty(),
                |this| {
                    this.child(
                        kit::icon_button(
                            "check-feeds",
                            IconName::RefreshCw,
                            "check every feed now",
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.run(RssAction::Refresh { id: None }, |_, _, _| {}, window, cx)
                        })),
                    )
                },
            )
    }

    fn render_feeds(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let list = self.rss.feeds.iter().map(|feed| {
            let active = self.selected_feed().map(|picked| &picked.id) == Some(&feed.id);
            let id = feed.id.clone();
            let menu_feed = feed.clone();
            let view = cx.entity().downgrade();
            let engine = self.engine.clone();
            div()
                .id(SharedString::from(format!("feed-{}", feed.id)))
                .h(px(32.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(4.))
                .cursor_pointer()
                .text_color(if !feed.enabled {
                    p.text_weaker
                } else if active {
                    p.text_strong
                } else {
                    p.text
                })
                .when(active, |this| this.bg(p.bg_hover))
                .when(!active, |this| this.hover(|style| style.bg(p.bg_weak)))
                .child(Icon::new(IconName::Rss).small().text_color(p.icon))
                .child(kit::truncate(feed.label().to_string()).flex_1())
                .map(|this| match &feed.error {
                    Some(_) => this.child(kit::dot(p.danger)),
                    None => this.child(
                        div()
                            .text_size(px(12.))
                            .text_color(p.text_weak)
                            .child(format::count(feed.items.len())),
                    ),
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.feed = Some(id.clone());
                    cx.notify();
                }))
                .context_menu(move |menu, window, cx| {
                    feed_menu(menu, &menu_feed, view.clone(), engine.clone(), window, cx)
                })
        });
        div()
            .w(px(260.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .border_r_1()
            .border_color(p.border_weak)
            .child(
                kit::input(&self.url)
                    .small()
                    .prefix(Icon::new(IconName::Plus).size(px(13.)).text_color(p.icon)),
            )
            .child(
                div()
                    .id("feed-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(list),
            )
    }

    fn render_items(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(feed) = self.selected_feed() else {
            return placeholder("add a feed address to start", p);
        };
        let now = bawkbit_core::now();
        let header = div()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .border_b_1()
            .border_color(p.border_weak)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        kit::truncate(feed.label().to_string())
                            .flex_1()
                            .text_color(p.text_strong),
                    )
                    .child(div().text_size(px(12.)).text_color(p.text_weak).child(
                        match feed.checked {
                            Some(at) => format!("checked {} ago", format::age(Some(at), now)),
                            None => "not checked yet".into(),
                        },
                    )),
            )
            .when_some(feed.error.clone(), |this, error| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(p.danger)
                        .child(kit::dot(p.danger))
                        .child(kit::truncate(error)),
                )
            });
        let rows = feed
            .items
            .iter()
            .enumerate()
            .map(|(ix, item)| item_row(ix, item, now, p, cx).into_any_element())
            .collect::<Vec<_>>();
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(if feed.items.is_empty() {
                placeholder("nothing in this feed yet", p)
            } else {
                div()
                    .id("feed-items")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .into_any_element()
            })
            .into_any_element()
    }

    fn render_rule_list(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.editor.as_ref().map(|editor| editor.id.clone());
        let now = bawkbit_core::now();
        let list = self.rss.rules.iter().map(|rule| {
            let active = editing.as_ref() == Some(&rule.id);
            let picked = rule.clone();
            div()
                .id(SharedString::from(format!("rule-{}", rule.id)))
                .h(px(40.))
                .px(px(10.))
                .flex()
                .flex_col()
                .justify_center()
                .rounded(px(4.))
                .cursor_pointer()
                .when(active, |this| this.bg(p.bg_hover))
                .when(!active, |this| this.hover(|style| style.bg(p.bg_weak)))
                .child(
                    kit::truncate(rule.name.clone()).text_color(if rule.enabled {
                        p.text_strong
                    } else {
                        p.text_weaker
                    }),
                )
                .child(div().text_size(px(11.)).text_color(p.text_weak).child(
                    match (rule.enabled, rule.last_match) {
                        (false, _) => "off".to_string(),
                        (true, Some(at)) => format!("took one {} ago", format::age(Some(at), now)),
                        (true, None) => "nothing yet".to_string(),
                    },
                ))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.edit(picked.clone(), window, cx)),
                )
        });
        div()
            .w(px(260.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .border_r_1()
            .border_color(p.border_weak)
            .child(
                kit::button("new-rule", cx)
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .label("new rule")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.edit(Rule::default(), window, cx)),
                    ),
            )
            .child(
                div()
                    .id("rule-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(list),
            )
    }

    fn render_editor(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = &self.editor else {
            return placeholder(
                "rules add new feed items whose titles match. pick one or make a new one.",
                p,
            );
        };
        let row = |label: &'static str, control: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(
                    div()
                        .w(px(140.))
                        .flex_none()
                        .text_color(p.text)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().max_w(px(520.)).child(control))
        };
        let toggle =
            |id: &'static str, label: &'static str, on: bool, set: fn(&mut Editor, bool)| {
                Switch::new(id)
                    .checked(on)
                    .label(label)
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        if let Some(editor) = &mut this.editor {
                            set(editor, *checked);
                        }
                        this.refresh_preview(cx);
                        cx.notify();
                    }))
            };
        let feeds = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(self.rss.feeds.iter().map(|feed| {
                let id = feed.id.clone();
                kit::chip(
                    SharedString::from(format!("rule-feed-{}", feed.id)),
                    feed.label().to_string(),
                    editor.feeds.contains(&feed.id),
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(editor) = &mut this.editor {
                        if let Some(at) = editor.feeds.iter().position(|feed| *feed == id) {
                            editor.feeds.remove(at);
                        } else {
                            editor.feeds.push(id.clone());
                        }
                    }
                    this.refresh_preview(cx);
                    cx.notify();
                }))
            }));
        let preview: AnyElement = match &editor.preview {
            Err(error) => div()
                .flex()
                .items_center()
                .gap_2()
                .text_color(p.danger)
                .child(kit::dot(p.danger))
                .child(error.clone())
                .into_any_element(),
            Ok(matches) if matches.is_empty() => div()
                .text_color(p.text_weaker)
                .child("nothing in the feeds matches right now")
                .into_any_element(),
            Ok(matches) => div()
                .flex()
                .flex_col()
                .children(matches.iter().take(50).map(|found| {
                    let feed = self
                        .rss
                        .feeds
                        .iter()
                        .find(|feed| feed.id == found.feed)
                        .map(|feed| feed.label().to_string())
                        .unwrap_or_default();
                    div()
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap_3()
                        .border_b_1()
                        .border_color(p.border_weak)
                        .text_size(px(12.))
                        .child(
                            kit::truncate(found.item.title.clone())
                                .flex_1()
                                .text_color(p.text),
                        )
                        .child(
                            kit::truncate(feed)
                                .w(px(140.))
                                .flex_none()
                                .text_color(p.text_weak),
                        )
                }))
                .into_any_element(),
        };
        div()
            .id("rule-editor")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(row("name", kit::input(&editor.name).into_any_element()))
            .child(row("must contain", kit::input(&editor.must).into_any_element()))
            .child(row("must not contain", kit::input(&editor.must_not).into_any_element()))
            .child(
                div()
                    .pl(px(156.))
                    .text_size(px(12.))
                    .text_color(p.text_weak)
                    .child("every word must be in the title. | separates choices and * matches anything."),
            )
            .child(row(
                "",
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(toggle("rule-on", "on", editor.enabled, |e, on| e.enabled = on))
                    .child(toggle("rule-regex", "regular expressions", editor.regex, |e, on| {
                        e.regex = on
                    }))
                    .child(toggle("rule-smart", "each episode once", editor.smart, |e, on| {
                        e.smart = on
                    }))
                    .child(toggle("rule-paused", "add paused", editor.paused, |e, on| {
                        e.paused = on
                    }))
                    .into_any_element(),
            ))
            .child(row(
                "feeds",
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(feeds)
                    .when(editor.feeds.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(12.))
                                .text_color(p.text_weak)
                                .child("none picked watches every feed"),
                        )
                    })
                    .into_any_element(),
            ))
            .child(row("category", kit::input(&editor.category).into_any_element()))
            .child(row("save to", kit::input(&editor.save_path).into_any_element()))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .pl(px(156.))
                    .child(
                        Button::new("save-rule")
                            .primary()
                            .small()
                            .label("save")
                            .on_click(cx.listener(|this, _, window, cx| this.save_rule(window, cx))),
                    )
                    .child(
                        kit::button("remove-rule", cx)
                            .small()
                            .label(if editor.id.is_empty() { "discard" } else { "remove" })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.remove_rule(window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .pt_2()
                    .text_size(px(12.))
                    .text_color(p.text_weak)
                    .child("matches in the feeds now; a rule only takes items that arrive later"),
            )
            .child(preview)
            .into_any_element()
    }
}

fn item_row(
    ix: usize,
    item: &FeedItem,
    now: i64,
    p: &Palette,
    cx: &mut Context<RssView>,
) -> impl IntoElement {
    let link = item.link.clone();
    let menu_item = item.clone();
    let view = cx.entity().downgrade();
    let can_add = !item.link.is_empty();
    div()
        .id(("feed-item", ix))
        .w_full()
        .h(px(ROW))
        .px_3()
        .flex()
        .items_center()
        .gap_3()
        .border_b_1()
        .border_color(p.border_weak)
        .text_size(px(12.))
        .when(can_add, |this| {
            this.cursor_pointer()
                .hover(|style| style.bg(p.bg_weak))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(RssEvent::Add(link.clone()))))
        })
        .context_menu(move |menu, _, _| item_menu(menu, &menu_item, view.clone()))
        .child(
            kit::truncate(item.title.clone())
                .flex_1()
                .text_size(px(13.))
                .text_color(p.text_strong),
        )
        .when_some(item.rule.clone(), |this, rule| {
            let tip: SharedString = format!("added by {rule}").into();
            this.child(
                div()
                    .id(("taken", ix))
                    .flex_none()
                    .child(
                        Icon::new(IconName::CircleCheck)
                            .size(px(14.))
                            .text_color(p.green),
                    )
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx)),
            )
        })
        .child(cell(
            item.size.map(format::bytes).unwrap_or_default(),
            76.,
            p,
        ))
        .child(cell(format::age(item.published, now), 48., p))
}

fn cell(text: String, width: f32, p: &Palette) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .flex()
        .justify_end()
        .whitespace_nowrap()
        .text_color(p.text_weak)
        .child(text)
}

fn item_menu(menu: PopupMenu, item: &FeedItem, view: WeakEntity<RssView>) -> PopupMenu {
    let mut menu = menu.min_w(px(180.));
    if !item.link.is_empty() {
        let (link, copy) = (item.link.clone(), item.link.clone());
        menu = menu
            .item(
                PopupMenuItem::new("add")
                    .icon(Icon::new(IconName::Plus))
                    .on_click(move |_, _, cx| {
                        let _ = view.update(cx, |_, cx| cx.emit(RssEvent::Add(link.clone())));
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
    }
    match item.page.clone() {
        Some(page) => menu.item(
            PopupMenuItem::new("open page")
                .icon(Icon::new(IconName::ExternalLink))
                .on_click(move |_, _, cx| cx.open_url(&page)),
        ),
        None => menu,
    }
}

fn feed_menu(
    menu: PopupMenu,
    feed: &Feed,
    view: WeakEntity<RssView>,
    engine: Entity<Engine>,
    _: &mut Window,
    _: &mut Context<PopupMenu>,
) -> PopupMenu {
    let action = |action: RssAction| {
        let view = view.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let action = action.clone();
            let _ = view.update(cx, |this, cx| this.run(action, |_, _, _| {}, window, cx));
        }
    };
    let rename_feed = feed.clone();
    menu.min_w(px(180.))
        .item(
            PopupMenuItem::new("check now")
                .icon(Icon::new(IconName::RefreshCw))
                .on_click(action(RssAction::Refresh {
                    id: Some(feed.id.clone()),
                })),
        )
        .item(
            PopupMenuItem::new("rename")
                .icon(Icon::new(IconName::Pencil))
                .on_click(move |_, window, cx| {
                    prompts::rename_feed(engine.clone(), &rename_feed, window, cx)
                }),
        )
        .item(
            PopupMenuItem::new(if feed.enabled { "turn off" } else { "turn on" })
                .icon(Icon::new(if feed.enabled {
                    IconName::Pause
                } else {
                    IconName::Play
                }))
                .on_click(action(RssAction::UpdateFeed {
                    id: feed.id.clone(),
                    name: feed.name.clone(),
                    enabled: !feed.enabled,
                })),
        )
        .item(
            PopupMenuItem::new("copy address")
                .icon(Icon::new(IconName::Copy))
                .on_click({
                    let url = feed.url.clone();
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))
                }),
        )
        .separator()
        .item(
            PopupMenuItem::new("remove feed")
                .icon(Icon::new(IconName::Trash))
                .on_click(action(RssAction::RemoveFeed {
                    id: feed.id.clone(),
                })),
        )
}

fn placeholder(text: &'static str, p: &Palette) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .px_8()
        .text_center()
        .text_color(p.text_weaker)
        .child(text)
        .into_any_element()
}

impl Render for RssView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let body = match self.tab {
            Tab::Feeds => div()
                .size_full()
                .flex()
                .child(self.render_feeds(&p, cx))
                .child(self.render_items(&p, cx)),
            Tab::Rules => div()
                .size_full()
                .flex()
                .child(self.render_rule_list(&p, cx))
                .child(self.render_editor(&p, cx)),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_tabs(&p, cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}
