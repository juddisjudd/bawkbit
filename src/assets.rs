use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(
    AppIcons,
    [
        Activity,
        AppWindow,
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
        FileDown,
        FileUp,
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
        Pencil,
        Play,
        Plus,
        RefreshCw,
        Rss,
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

mod flags {
    include!(concat!(env!("OUT_DIR"), "/flags.rs"));
}

/// The asset path of a country's flag, if bawkbit has one.
pub fn flag(code: &str) -> Option<SharedString> {
    flag_svg(code).map(|_| format!("flags/{code}.svg").into())
}

fn flag_svg(code: &str) -> Option<&'static [u8]> {
    flags::FLAGS
        .binary_search_by(|(known, _)| (*known).cmp(code))
        .ok()
        .map(|ix| flags::FLAGS[ix].1)
}

/// GPUI draws an SVG image at the size it states, and the flags state none, so each gets a small one.
fn sized_flag(svg: &[u8]) -> Vec<u8> {
    let mut sized = b"<svg width=\"36\" height=\"24\"".to_vec();
    sized.extend_from_slice(svg.strip_prefix(b"<svg").unwrap_or(svg));
    sized
}

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == MARK {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/brand/mark.svg"
            ))));
        }
        if let Some(code) = path
            .strip_prefix("flags/")
            .and_then(|name| name.strip_suffix(".svg"))
        {
            return Ok(flag_svg(code).map(|svg| Cow::Owned(sized_flag(svg))));
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
