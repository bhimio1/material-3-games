//! User interface: the desktop window's pages and the big-picture mode.

pub mod art;
pub mod bigpicture;
pub mod common;
pub mod pages;
pub mod sidebar;
pub mod steamui;
pub mod toasts;
pub mod widgets;

use crate::library::Source;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

/// Library filter in the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Recent,
    Installed,
    Source(Source),
    /// Desktop entries and games added by hand.
    Other,
}

impl Filter {
    pub fn title(self) -> &'static str {
        match self {
            Filter::All => "All games",
            Filter::Recent => "Recently played",
            Filter::Installed => "Installed",
            Filter::Source(Source::Steam) => "Steam",
            Filter::Source(Source::Lutris) => "Lutris",
            Filter::Source(Source::Heroic) => "Heroic",
            Filter::Source(_) | Filter::Other => "Other games",
        }
    }

    pub fn matches(self, g: &crate::library::Game) -> bool {
        match self {
            Filter::All => true,
            Filter::Recent => g.last_played.is_some(),
            Filter::Installed => g.installed,
            Filter::Source(s) => g.source == s,
            Filter::Other => matches!(g.source, Source::Desktop | Source::Manual),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Library(Filter),
    Detail(String),
    /// Add a game, or edit the manual game with this slug.
    Add(Option<String>),
    GameMode,
    Settings,
}

#[derive(Clone, Debug)]
pub enum Action {
    Goto(Page),
    Back,
    Toast(String, ToastKind),
    Launch(String),
    Install { packages: Vec<String>, aur: bool },
    OpenPath(PathBuf),
    OpenUrl(String),
    Rescan,
    FetchCovers,
    ConfigChanged,
    /// Ask before entering Game Mode (lists the windows that will close).
    ConfirmGameMode,
    EnterGameMode,
    InstallRules(bool),
    InstallSession,
    /// A change to Steam's files (asks to restart Steam if it's running).
    Steam(SteamJob),
    /// Same, after the user agreed to restart Steam.
    SteamConfirmed(SteamJob),
    Quit,
}

/// Edits to Steam's own files. Steam overwrites them when it exits, so they
/// run with Steam closed.
#[derive(Clone, Debug, PartialEq)]
pub enum SteamJob {
    /// Add/remove non-Steam shortcuts (Steam Input) for these game ids.
    Shortcuts { add: Vec<String>, remove: Vec<String> },
    /// Compatibility tool for a Steam game ("" = Steam's default).
    Compat { appid: String, tool: String },
    LaunchOptions { appid: String, options: String },
}

impl SteamJob {
    pub fn describe(&self) -> String {
        match self {
            SteamJob::Shortcuts { add, remove } => match (add.len(), remove.len()) {
                (a, 0) => format!("Add {a} game{} to Steam for Steam Input", if a == 1 { "" } else { "s" }),
                (0, r) => format!("Remove {r} game{} from Steam", if r == 1 { "" } else { "s" }),
                (a, r) => format!("Add {a} and remove {r} Steam shortcuts"),
            },
            SteamJob::Compat { .. } => "Change the compatibility tool".into(),
            SteamJob::LaunchOptions { .. } => "Change the Steam launch options".into(),
        }
    }
}

pub type Actions = Vec<Action>;

/// "3 h 20 min", "45 min", "Never played".
pub fn format_playtime(minutes: Option<u64>) -> String {
    match minutes.unwrap_or(0) {
        0 => "Never played".into(),
        m if m < 60 => format!("{m} min"),
        m if m % 60 == 0 => format!("{} h", m / 60),
        m => format!("{} h {} min", m / 60, m % 60),
    }
}

/// "Today", "Yesterday", "3 days ago", or a date.
pub fn format_last_played(ts: Option<i64>, now: i64) -> String {
    let Some(ts) = ts else {
        return "Never".into();
    };
    let days = (now - ts).max(0) / 86_400;
    match days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => format!("{days} days ago"),
        _ => chrono::DateTime::from_timestamp(ts, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string())
            .unwrap_or_else(|| "Long ago".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting() {
        assert_eq!(format_playtime(None), "Never played");
        assert_eq!(format_playtime(Some(45)), "45 min");
        assert_eq!(format_playtime(Some(120)), "2 h");
        assert_eq!(format_playtime(Some(200)), "3 h 20 min");
        let now = 1_700_000_000;
        assert_eq!(format_last_played(Some(now - 100), now), "Today");
        assert_eq!(format_last_played(Some(now - 86_400 * 3), now), "3 days ago");
        assert_eq!(format_last_played(None, now), "Never");
    }
}
