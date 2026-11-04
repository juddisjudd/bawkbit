use std::path::Path;

use bawkbit_core::{AddRequest, Id, Preview, Source};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::kit;
use crate::engine::Engine;
use crate::format;
use crate::theme::{Palette, palette};

/// The dialog is finished; the id is the torrent to show, if any.
pub struct AddDone(pub Option<Id>);

enum Stage {
    Input,
    Resolving(String),
    Ready(Preview),
}

pub struct AddView {
    engine: Entity<Engine>,
    stage: Stage,
    link: Entity<InputState>,
    save_path: Entity<InputState>,
    category: Entity<InputState>,
    new_tags: Entity<InputState>,
    tags: Vec<String>,
    files: Vec<bool>,
    start: bool,
    skip_check: bool,
    busy: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AddDone> for AddView {}

impl AddView {
    pub fn new(
        engine: Entity<Engine>,
        source: Option<Source>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let link =
            cx.new(|cx| InputState::new(window, cx).placeholder("magnet link or torrent url"));
        let save_path = cx.new(|cx| InputState::new(window, cx));
        let category = cx.new(|cx| InputState::new(window, cx).placeholder("category"));
        let new_tags = cx.new(|cx| InputState::new(window, cx).placeholder("tags"));
        let start = !engine.read(cx).core().settings().start_paused;
        let subscriptions = vec![
            cx.subscribe_in(
                &link,
                window,
                |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        let text = input.read(cx).value().trim().to_string();
                        if !text.is_empty() {
                            this.resolve(Source::Link(text), window, cx);
                        }
                    }
                },
            ),
            cx.subscribe_in(
                &category,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.update_path_hint(window, cx);
                    }
                },
            ),
        ];
        let mut this = Self {
            engine,
            stage: Stage::Input,
            link,
            save_path,
            category,
            new_tags,
            tags: Vec::new(),
            files: Vec::new(),
            start,
            skip_check: false,
            busy: false,
            error: None,
            _subscriptions: subscriptions,
        };
        if let Some(source) = source {
            this.resolve(source, window, cx);
        }
        this
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.link.focus_handle(cx)
    }

    fn resolve(&mut self, source: Source, window: &mut Window, cx: &mut Context<Self>) {
        let label = match &source {
            Source::Link(link) => link_name(link),
            Source::File(_) | Source::Resolved(_) => "reading the torrent".into(),
        };
        self.stage = Stage::Resolving(label);
        self.error = None;
        let job = self
            .engine
            .read(cx)
            .run(move |core| async move { core.preview(source).await });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(preview) => {
                        this.files = vec![true; preview.files.len()];
                        this.stage = Stage::Ready(preview);
                        this.update_path_hint(window, cx);
                    }
                    Err(err) => {
                        this.stage = Stage::Input;
                        this.error = Some(format!("{err:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn pick_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("add".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| match std::fs::read(&path) {
                Ok(bytes) => this.resolve(Source::File(bytes), window, cx),
                Err(err) => {
                    this.error = Some(format!("cannot read {}: {err}", path.display()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("save here".into()),
        });
        let input = self.save_path.clone();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                let _ = cx.update(|window, cx| {
                    input.update(cx, |state, cx| {
                        state.set_value(path.to_string_lossy().into_owned(), window, cx)
                    })
                });
            }
        })
        .detach();
    }

    /// Shows where the files go when the path is left blank: the category's folder or the download folder.
    fn update_path_hint(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let category = self.category.read(cx).value().trim().to_string();
        let settings = self.engine.read(cx).core().settings();
        let custom = self
            .engine
            .read(cx)
            .snapshot
            .categories
            .iter()
            .find(|c| c.name == category)
            .map(|c| c.save_path.clone())
            .filter(|path| !path.is_empty());
        let hint = match (category.is_empty(), custom) {
            (true, _) => settings.download_dir,
            (false, Some(path)) => path,
            (false, None) => Path::new(&settings.download_dir)
                .join(&category)
                .to_string_lossy()
                .into_owned(),
        };
        self.save_path
            .update(cx, |state, cx| state.set_placeholder(hint, window, cx));
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Stage::Ready(preview) = &self.stage else {
            return;
        };
        let files: Vec<usize> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, on)| **on)
            .map(|(ix, _)| ix)
            .collect();
        if files.is_empty() {
            self.error = Some("pick at least one file.".into());
            cx.notify();
            return;
        }
        let mut tags = self.tags.clone();
        tags.extend(
            self.new_tags
                .read(cx)
                .value()
                .split(',')
                .map(|tag| tag.trim().to_string())
                .filter(|tag| !tag.is_empty()),
        );
        let text = |input: &Entity<InputState>| {
            Some(input.read(cx).value().trim().to_string()).filter(|value| !value.is_empty())
        };
        let request = AddRequest {
            source: Source::Resolved(preview.id.clone()),
            save_path: text(&self.save_path),
            category: text(&self.category),
            tags,
            paused: Some(!self.start),
            files: (files.len() < self.files.len()).then_some(files),
            skip_check: self.skip_check,
        };
        self.busy = true;
        self.error = None;
        let job = self
            .engine
            .read(cx)
            .run(move |core| async move { core.add(request).await });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(id) => cx.emit(AddDone(Some(id))),
                    Err(err) => this.error = Some(format!("{err:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn render_input(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div().flex_1().child(
                            kit::input(&self.link).prefix(
                                Icon::new(IconName::Magnet).size(px(14.)).text_color(p.icon),
                            ),
                        ),
                    )
                    .child(
                        kit::button("pick-file", cx)
                            .icon(Icon::new(IconName::File))
                            .label("open file")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.pick_file(window, cx)),
                            ),
                    ),
            )
            .child(div().text_size(px(12.)).text_color(p.text_weaker).child(
                "you can also drop .torrent files on the window or paste a magnet link anywhere.",
            ))
    }

    fn render_ready(&self, preview: &Preview, p: &Palette, cx: &mut Context<Self>) -> Div {
        let selected: u64 = preview
            .files
            .iter()
            .zip(&self.files)
            .filter(|(_, on)| **on)
            .map(|(file, _)| file.size)
            .sum();
        let all = self.files.iter().all(|on| *on);
        let categories = self.engine.read(cx).snapshot.categories.clone();
        let known_tags = self.engine.read(cx).snapshot.tags.clone();
        let current_category = self.category.read(cx).value().trim().to_string();

        let files =
            (preview.files.len() > 1).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(p.border_weak)
                    .rounded(px(6.))
                    .child(
                        div()
                            .h(px(32.))
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .border_b_1()
                            .border_color(p.border_weak)
                            .text_size(px(12.))
                            .text_color(p.text_weak)
                            .child(
                                Checkbox::new("all-files")
                                    .checked(all)
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        this.files.iter_mut().for_each(|on| *on = *checked);
                                        cx.notify();
                                    })),
                            )
                            .child(format!(
                                "{} of {} files",
                                self.files.iter().filter(|on| **on).count(),
                                preview.files.len()
                            )),
                    )
                    .child(
                        div()
                            .id("preview-files")
                            .max_h(px(220.))
                            .overflow_y_scroll()
                            .children(preview.files.iter().map(|file| {
                                let index = file.index;
                                let on = self.files.get(index).copied().unwrap_or(true);
                                div()
                                    .h(px(28.))
                                    .px_3()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .text_size(px(12.))
                                    .child(Checkbox::new(("pick", index)).checked(on).on_click(
                                        cx.listener(move |this, checked: &bool, _, cx| {
                                            if let Some(slot) = this.files.get_mut(index) {
                                                *slot = *checked;
                                            }
                                            cx.notify();
                                        }),
                                    ))
                                    .child(
                                        kit::truncate(file.path.clone()).flex_1().text_color(
                                            if on { p.text_strong } else { p.text_weaker },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(p.text_weak)
                                            .child(format::bytes(file.size)),
                                    )
                            })),
                    )
            });

        let row = |icon: IconName| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(Icon::new(icon).size(px(15.)).text_color(p.icon).flex_none())
        };

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_3()
                    .child(
                        kit::truncate(preview.name.clone())
                            .flex_1()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_strong),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(p.text_weak)
                            .child(format::bytes(selected)),
                    ),
            )
            .when(preview.existing, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(p.warning)
                        .child(kit::dot(p.warning))
                        .child("this torrent is already in the list."),
                )
            })
            .children(files)
            .child(
                row(IconName::HardDrive)
                    .child(div().flex_1().child(kit::input(&self.save_path)))
                    .child(
                        kit::icon_button("browse-save", IconName::FolderOpen, "pick a folder")
                            .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                    ),
            )
            .child(
                row(IconName::Folder)
                    .child(
                        field_box(categories.is_empty()).child(kit::input(&self.category).small()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .children(categories.into_iter().map(|category| {
                                let active = category.name == current_category;
                                let name = category.name.clone();
                                kit::chip(
                                    SharedString::from(format!("cat-{name}")),
                                    name.clone(),
                                    active,
                                    p,
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        let value =
                                            if active { String::new() } else { name.clone() };
                                        this.category.update(cx, |state, cx| {
                                            state.set_value(value, window, cx)
                                        });
                                        this.update_path_hint(window, cx);
                                    },
                                ))
                            })),
                    ),
            )
            .child(
                row(IconName::Tag)
                    .child(
                        field_box(known_tags.is_empty()).child(kit::input(&self.new_tags).small()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .children(known_tags.into_iter().map(|tag| {
                                let active = self.tags.contains(&tag);
                                let name = tag.clone();
                                kit::chip(SharedString::from(format!("tag-{tag}")), tag, active, p)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if active {
                                            this.tags.retain(|t| *t != name);
                                        } else {
                                            this.tags.push(name.clone());
                                        }
                                        cx.notify();
                                    }))
                            })),
                    ),
            )
    }
}

/// A text box that fills the row unless chips sit next to it.
fn field_box(alone: bool) -> Div {
    if alone {
        div().flex_1()
    } else {
        div().w(px(180.)).flex_none()
    }
}

/// A magnet's display name, or the link itself, to show while the metadata arrives.
fn link_name(link: &str) -> String {
    link.split(['?', '&'])
        .find_map(|pair| pair.strip_prefix("dn="))
        .map(|name| name.replace('+', " ").replace("%20", " "))
        .unwrap_or_else(|| link.chars().take(80).collect())
}

impl Render for AddView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let ready = matches!(self.stage, Stage::Ready(_));
        let body = match &self.stage {
            Stage::Input => self.render_input(&p, cx),
            Stage::Resolving(label) => div()
                .h(px(120.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(gpui_kit::component::spinner::Spinner::new().color(p.yolk))
                .child(
                    kit::truncate(label.clone())
                        .max_w(px(560.))
                        .text_color(p.text_strong),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.text_weaker)
                        .child("asking peers for the file list…"),
                ),
            Stage::Ready(preview) => {
                let preview = preview.clone();
                self.render_ready(&preview, &p, cx)
            }
        };
        let existing = match &self.stage {
            Stage::Ready(preview) if preview.existing => Some(preview.id.clone()),
            _ => None,
        };

        div()
            .flex()
            .flex_col()
            .gap_4()
            .pt_2()
            .child(body)
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(p.danger)
                        .child(kit::dot(p.danger))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(ready && existing.is_none(), |this| {
                        this.child(
                            Switch::new("start-now")
                                .checked(self.start)
                                .label("start now")
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.start = *checked;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Switch::new("skip-check")
                                .checked(self.skip_check)
                                .label("skip check")
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.skip_check = *checked;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        kit::button("cancel-add", cx)
                            .label("cancel")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(AddDone(None)))),
                    )
                    .map(|this| match existing {
                        Some(id) => this.child(
                            Button::new("show-existing")
                                .primary()
                                .label("show it")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(AddDone(Some(id.clone())))
                                })),
                        ),
                        None => this.child(
                            Button::new("confirm-add")
                                .primary()
                                .label("add")
                                .loading(self.busy)
                                .disabled(!ready || self.busy)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.submit(window, cx)),
                                ),
                        ),
                    }),
            )
    }
}
