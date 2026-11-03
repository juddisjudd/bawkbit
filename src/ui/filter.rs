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
