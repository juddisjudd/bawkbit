use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(
    AppIcons,
    [
        Activity,
        ArrowDown,
        ArrowUp,
        CircleAlert,
        CircleCheck,
        CirclePause,
        Clock,
        Copy,
        Ban,
        Download,
        ExternalLink,
        FastForward,
        File,
        Folder,
        FolderDown,
        FolderOpen,
        Gauge,
        Globe,
        HardDrive,
        Hash,
        Hourglass,
        Info,
        Layers,
        Link,
        ListFilter,
        ListPlus,
        LoaderCircle,
        Magnet,
        Network,
        Palette,
        Pause,
        Play,
        Plus,
        RefreshCw,
        Search,
        Server,
        Settings,
        Share2,
        SlidersHorizontal,
        Tag,
        Trash,
        Turtle,
        Upload,
        Users,
        Wifi,
        X,
    ]
);

pub const MARK: &str = "brand/mark.svg";

pub const FONTS: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Bold.ttf"),
];

/// GPUI takes IBM Plex Sans for the Linux system font and panics at startup when neither it nor a few common fonts are installed.
#[cfg(target_os = "linux")]
pub const SYSTEM_FONT: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf");

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == MARK {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/brand/mark.svg"
            ))));
        }
        if let Some(bytes) = AppIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(AppIcons.list(path)?);
        if MARK.starts_with(path) {
            paths.push(MARK.into());
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
