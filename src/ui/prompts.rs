use std::rc::Rc;

use bawkbit_core::{Action, Id};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Icon, WindowExt};
use gpui_kit::*;

use super::kit;
use crate::engine::Engine;
use crate::format;

/// Closes the dialog only once `action` worked, so an error leaves what was typed in place.
fn submit(engine: &Entity<Engine>, action: Action, window: &mut Window, cx: &mut App) {
    let job = engine
        .read(cx)
        .run(move |core| async move { core.execute(action).await.map(|_| ()) });
    window
        .spawn(cx, async move |cx| {
            let result = job.await;
            let _ = cx.update(|window, cx| match result {
                Ok(()) => window.close_dialog(cx),
                Err(err) => window.push_notification(Notification::error(format!("{err:#}")), cx),
            });
        })
        .detach();
}

fn footer(
    id: &'static str,
    label: &'static str,
    save: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(
            kit::button(SharedString::from(format!("cancel-{id}")), cx)
                .label("cancel")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(
            Button::new(SharedString::from(format!("save-{id}")))
                .primary()
                .label(label)
                .on_click(move |_, window, cx| save(window, cx)),
        )
}

/// Adds trackers to one torrent; it keeps its pieces and carries on.
pub fn add_trackers(engine: Entity<Engine>, id: Id, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| {
        TextareaState::new(window, cx)
            .auto_grow(4, 10)
            .placeholder("tracker addresses, one per line")
    });
    let save = {
        let input = input.clone();
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let typed: Vec<String> = input
                .read(cx)
                .value()
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(String::from)
                .collect();
            if typed.is_empty() {
                return;
            }
            let mut trackers = engine
                .read(cx)
                .core()
                .details(&id)
                .map(|details| details.trackers)
                .unwrap_or_default();
            trackers.extend(typed);
            let action = Action::SetTrackers {
                id: id.clone(),
                trackers,
            };
            submit(&engine, action, window, cx);
        })
    };
    let focus = input.focus_handle(cx);
    window.open_dialog(cx, move |dialog, _, cx| {
        let save = save.clone();
        dialog
            .w(px(520.))
            .child(Textarea::new(&input).w_full())
            .footer(footer(
                "trackers",
                "add",
                move |window, cx| save(window, cx),
                cx,
            ))
    });
    window.focus(&focus, cx);
}

/// Download and upload limits for the picked torrents; `current` fills the boxes when they all share one.
pub fn speed_limits(
    engine: Entity<Engine>,
    ids: Vec<Id>,
    current: Option<(u64, u64)>,
    window: &mut Window,
    cx: &mut App,
) {
    let (down_now, up_now) = current.unwrap_or_default();
    let field = |value: u64, window: &mut Window, cx: &mut App| {
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("no limit")
                .default_value(format::rate_input(value))
        })
    };
    let down = field(down_now, window, cx);
    let up = field(up_now, window, cx);
    let save = {
        let (down, up) = (down.clone(), up.clone());
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let read = |input: &Entity<InputState>| format::parse_rate(&input.read(cx).value());
            let (Some(down), Some(up)) = (read(&down), read(&up)) else {
                let note = Notification::warning("use a speed like 500 KB or 2 MB");
                window.push_notification(note, cx);
                return;
            };
            let action = Action::SetLimits {
                ids: ids.clone(),
                down,
                up,
            };
            submit(&engine, action, window, cx);
        })
    };
    let handle = window.window_handle();
    for input in [&down, &up] {
        let save = save.clone();
        cx.subscribe(input, move |_, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let save = save.clone();
                let _ = handle.update(cx, |_, window, cx| save(window, cx));
            }
        })
        .detach();
    }
    let focus = down.focus_handle(cx);
    window.open_dialog(cx, move |dialog, _, cx| {
        let save = save.clone();
        let speed = |input: &Entity<InputState>, icon: IconName| {
            kit::input(input).prefix(Icon::new(icon).text_color(crate::theme::palette(cx).icon))
        };
        dialog
            .w(px(360.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(speed(&down, IconName::ArrowDown))
                    .child(speed(&up, IconName::ArrowUp)),
            )
            .footer(footer(
                "limits",
                "save",
                move |window, cx| save(window, cx),
                cx,
            ))
    });
    window.focus(&focus, cx);
}
