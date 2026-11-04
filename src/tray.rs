pub enum TrayCommand {
    Open,
    PauseAll,
    ResumeAll,
    ToggleAltSpeed,
    Quit,
}

const OPEN: &str = "open bawkbit";
const PAUSE_ALL: &str = "pause all";
const RESUME_ALL: &str = "resume all";
const ALT_SPEED: &str = "alternative speeds";
const QUIT: &str = "quit bawkbit";

/// The icon in the system tray, with a menu for the things worth doing without the window.
pub struct Tray {
    inner: imp::Tray,
    tooltip: String,
    alt_speed: bool,
}

impl Tray {
    /// `None` when the desktop has no tray to show it in.
    pub fn new(
        commands: async_channel::Sender<TrayCommand>,
        runtime: &tokio::runtime::Handle,
    ) -> Option<Self> {
        Some(Self {
            inner: imp::Tray::new(commands, runtime)?,
            tooltip: String::new(),
            alt_speed: false,
        })
    }

    pub fn update(&mut self, tooltip: String, alt_speed: bool) {
        if self.tooltip != tooltip || self.alt_speed != alt_speed {
            self.inner.update(&tooltip, alt_speed);
            self.tooltip = tooltip;
            self.alt_speed = alt_speed;
        }
    }
}

fn icon_pixels(size: u32) -> Option<Vec<u8>> {
    const PNG: &[u8] = include_bytes!("../assets/brand/bawkbit.png");
    let image = image::load_from_memory_with_format(PNG, image::ImageFormat::Png)
        .map_err(|err| log::error!("cannot read the tray icon: {err}"))
        .ok()?;
    let image = image.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
    Some(image.to_rgba8().into_raw())
}

#[cfg(windows)]
mod imp {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    use super::*;

    pub struct Tray {
        icon: TrayIcon,
        alt_speed: CheckMenuItem,
    }

    impl Tray {
        pub fn new(
            commands: async_channel::Sender<TrayCommand>,
            _runtime: &tokio::runtime::Handle,
        ) -> Option<Self> {
            let menu = Menu::new();
            let alt_speed = CheckMenuItem::with_id("alt-speed", ALT_SPEED, true, false, None);
            menu.append_items(&[
                &MenuItem::with_id("open", OPEN, true, None),
                &PredefinedMenuItem::separator(),
                &MenuItem::with_id("pause-all", PAUSE_ALL, true, None),
                &MenuItem::with_id("resume-all", RESUME_ALL, true, None),
                &alt_speed,
                &PredefinedMenuItem::separator(),
                &MenuItem::with_id("quit", QUIT, true, None),
            ])
            .map_err(|err| log::error!("cannot build the tray menu: {err}"))
            .ok()?;

            let sender = commands.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let command = match event.id().as_ref() {
                    "open" => TrayCommand::Open,
                    "pause-all" => TrayCommand::PauseAll,
                    "resume-all" => TrayCommand::ResumeAll,
                    "alt-speed" => TrayCommand::ToggleAltSpeed,
                    "quit" => TrayCommand::Quit,
                    _ => return,
                };
                let _ = sender.try_send(command);
            }));
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let _ = commands.try_send(TrayCommand::Open);
                }
            }));

            let icon = Icon::from_rgba(icon_pixels(32)?, 32, 32).ok()?;
            let icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .with_tooltip("bawkbit")
                .with_icon(icon)
                .build()
                .map_err(|err| log::error!("cannot show the tray icon: {err}"))
                .ok()?;
            Some(Self { icon, alt_speed })
        }

        pub fn update(&mut self, tooltip: &str, alt_speed: bool) {
            let _ = self.icon.set_tooltip(Some(tooltip));
            self.alt_speed.set_checked(alt_speed);
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use ksni::TrayMethods as _;
    use ksni::menu::{CheckmarkItem, StandardItem};

    use super::*;

    struct Item {
        commands: async_channel::Sender<TrayCommand>,
        icon: Vec<ksni::Icon>,
        tooltip: String,
        alt_speed: bool,
    }

    impl Item {
        fn send(&self, command: TrayCommand) {
            let _ = self.commands.try_send(command);
        }
    }

    fn entry(label: &str, command: fn() -> TrayCommand) -> ksni::MenuItem<Item> {
        StandardItem {
            label: label.into(),
            activate: Box::new(move |item: &mut Item| item.send(command())),
            ..Default::default()
        }
        .into()
    }

    impl ksni::Tray for Item {
        fn id(&self) -> String {
            "bawkbit".into()
        }

        fn title(&self) -> String {
            "bawkbit".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icon.clone()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "bawkbit".into(),
                description: self.tooltip.clone(),
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            self.send(TrayCommand::Open);
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            vec![
                entry(OPEN, || TrayCommand::Open),
                ksni::MenuItem::Separator,
                entry(PAUSE_ALL, || TrayCommand::PauseAll),
                entry(RESUME_ALL, || TrayCommand::ResumeAll),
                CheckmarkItem {
                    label: ALT_SPEED.into(),
                    checked: self.alt_speed,
                    activate: Box::new(|item: &mut Item| item.send(TrayCommand::ToggleAltSpeed)),
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
                entry(QUIT, || TrayCommand::Quit),
            ]
        }
    }

    pub struct Tray {
        handle: ksni::Handle<Item>,
        runtime: tokio::runtime::Handle,
    }

    impl Tray {
        pub fn new(
            commands: async_channel::Sender<TrayCommand>,
            runtime: &tokio::runtime::Handle,
        ) -> Option<Self> {
            let mut data = icon_pixels(64)?;
            for pixel in data.as_chunks_mut::<4>().0 {
                pixel.rotate_right(1);
            }
            let item = Item {
                commands,
                icon: vec![ksni::Icon {
                    width: 64,
                    height: 64,
                    data,
                }],
                tooltip: String::new(),
                alt_speed: false,
            };
            let handle = runtime
                .block_on(item.spawn())
                .map_err(|err| log::warn!("no tray icon: {err}"))
                .ok()?;
            Some(Self {
                handle,
                runtime: runtime.clone(),
            })
        }

        pub fn update(&mut self, tooltip: &str, alt_speed: bool) {
            let handle = self.handle.clone();
            let tooltip = tooltip.to_string();
            self.runtime.spawn(async move {
                handle
                    .update(|item| {
                        item.tooltip = tooltip;
                        item.alt_speed = alt_speed;
                    })
                    .await;
            });
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            drop(self.handle.shutdown());
        }
    }
}
