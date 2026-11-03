use bawkbit_core::{State, Torrent};
use gpui_kit::assets::IconName;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    All,
    Downloading,
    Seeding,
    Done,
    Paused,
    Active,
    Error,
}

impl Status {
    pub const ALL: [Status; 7] = [
        Status::All,
        Status::Downloading,
        Status::Seeding,
        Status::Done,
        Status::Paused,
        Status::Active,
        Status::Error,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Status::All => "all",
            Status::Downloading => "downloading",
            Status::Seeding => "seeding",
            Status::Done => "done",
            Status::Paused => "paused",
            Status::Active => "active",
            Status::Error => "error",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Status::All => IconName::Layers,
            Status::Downloading => IconName::ArrowDown,
            Status::Seeding => IconName::ArrowUp,
            Status::Done => IconName::CircleCheck,
            Status::Paused => IconName::Pause,
            Status::Active => IconName::Activity,
            Status::Error => IconName::CircleAlert,
        }
    }

    pub fn matches(self, torrent: &Torrent) -> bool {
        match self {
            Status::All => true,
            Status::Downloading => matches!(
                torrent.state,
                State::Downloading
                    | State::Stalled
                    | State::Metadata
                    | State::Checking
                    | State::Queued
            ),
            Status::Seeding => torrent.state == State::Seeding,
            Status::Done => torrent.state == State::Done,
            Status::Paused => torrent.state == State::Paused,
            Status::Active => torrent.down > 0 || torrent.up > 0,
            Status::Error => torrent.state == State::Error,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    pub status: Status,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub text: String,
}

impl Filters {
    pub fn matches(&self, torrent: &Torrent) -> bool {
        self.status.matches(torrent)
            && self
                .category
                .as_ref()
                .is_none_or(|category| torrent.category.as_ref() == Some(category))
            && self
                .tag
                .as_ref()
                .is_none_or(|tag| torrent.tags.contains(tag))
            && words_match(&self.text, &torrent.name)
    }
}

/// Every word typed appears somewhere in the name, in any order and case.
fn words_match(query: &str, name: &str) -> bool {
    let name = name.to_lowercase();
    query
        .split_whitespace()
        .all(|word| name.contains(&word.to_lowercase()))
}

pub fn state_icon(state: State) -> IconName {
    match state {
        State::Metadata => IconName::Magnet,
        State::Checking => IconName::RefreshCw,
        State::Downloading | State::Stalled => IconName::ArrowDown,
        State::Seeding => IconName::ArrowUp,
        State::Done => IconName::CircleCheck,
        State::Paused => IconName::Pause,
        State::Queued => IconName::Hourglass,
        State::Error => IconName::CircleAlert,
    }
}

pub fn state_label(state: State) -> &'static str {
    match state {
        State::Metadata => "finding metadata",
        State::Checking => "checking",
        State::Downloading => "downloading",
        State::Stalled => "waiting for peers",
        State::Seeding => "seeding",
        State::Done => "done",
        State::Paused => "paused",
        State::Queued => "queued",
        State::Error => "error",
    }
}

/// A local filter over search results: words to keep, `-words` to drop, and `>1GB` or `<500MB` size bounds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Refine {
    words: Vec<String>,
    excluded: Vec<String>,
    min: Option<u64>,
    max: Option<u64>,
}

impl Refine {
    pub fn parse(text: &str) -> Self {
        let mut refine = Refine::default();
        for token in text.split_whitespace() {
            let lower = token.to_lowercase();
            if let Some(size) = lower
                .strip_prefix(">=")
                .or_else(|| lower.strip_prefix('>'))
                .and_then(size)
            {
                refine.min = Some(size);
            } else if let Some(size) = lower
                .strip_prefix("<=")
                .or_else(|| lower.strip_prefix('<'))
                .and_then(size)
            {
                refine.max = Some(size);
            } else if let Some(word) = lower.strip_prefix('-').filter(|word| !word.is_empty()) {
                refine.excluded.push(word.to_string());
            } else {
                refine.words.push(lower);
            }
        }
        refine
    }

    pub fn matches(&self, title: &str, bytes: u64) -> bool {
        let title = title.to_lowercase();
        self.words.iter().all(|word| title.contains(word))
            && !self.excluded.iter().any(|word| title.contains(word))
            && self.min.is_none_or(|min| bytes >= min)
            && self.max.is_none_or(|max| bytes <= max)
    }
}

/// "1.5gb", "700mb" or a bare number of megabytes.
fn size(text: &str) -> Option<u64> {
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    let scale: f64 = match unit {
        "" | "m" | "mb" | "mib" => 1024.0 * 1024.0,
        "b" => 1.0,
        "k" | "kb" | "kib" => 1024.0,
        "g" | "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * scale) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrent(name: &str, state: State) -> Torrent {
        Torrent {
            id: "x".into(),
            name: name.into(),
            state,
            progress: 0.0,
            size: 0,
            done: 0,
            uploaded: 0,
            down: 0,
            up: 0,
            eta: None,
            ratio: 0.0,
            peers: 0,
            seeds: 0,
            category: Some("linux".into()),
            tags: vec!["iso".into()],
            added: 0,
            completed: None,
            save_path: String::new(),
            error: None,
        }
    }

    #[test]
    fn refine_keeps_words_drops_minus_words_and_bounds_size() {
        let refine = Refine::parse("ubuntu -beta >1gb <3GB");
        let gb = 1024 * 1024 * 1024;
        assert!(refine.matches("Ubuntu 24.04 Desktop", 2 * gb));
        assert!(!refine.matches("Ubuntu 25.04 Beta", 2 * gb));
        assert!(!refine.matches("Ubuntu Server", gb / 2));
        assert!(!refine.matches("Ubuntu DVD", 4 * gb));
        assert!(Refine::parse(">700").matches("x", 800 * 1024 * 1024));
        assert!(Refine::parse("").matches("anything", 0));
    }

    #[test]
    fn filters_combine() {
        let t = torrent("Debian 13 netinst", State::Queued);
        let mut filters = Filters {
            status: Status::Downloading,
            category: Some("linux".into()),
            tag: Some("iso".into()),
            text: "NETINST debian".into(),
        };
        assert!(filters.matches(&t));
        filters.tag = Some("other".into());
        assert!(!filters.matches(&t));
        filters.tag = None;
        filters.status = Status::Seeding;
        assert!(!filters.matches(&t));
    }
}
