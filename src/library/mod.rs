//! The game library: every source scanned into one list of [`Game`]s, our
//! own play statistics merged in, and the JSON export the end4-pC shell reads.

pub mod desktop;
pub mod heroic;
pub mod lutris;
pub mod manual;
pub mod steam;
pub mod vdf;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Steam,
    Lutris,
    Heroic,
    Desktop,
    Manual,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Steam => "Steam",
            Source::Lutris => "Lutris",
            Source::Heroic => "Heroic",
            Source::Desktop => "App",
            Source::Manual => "Added by you",
        }
    }
}

/// How a game is started.
#[derive(Clone, Debug, PartialEq)]
pub enum Launch {
    Steam { appid: String },
    Lutris { id: i64 },
    Heroic { runner: String, app_name: String },
    /// A desktop entry's Exec line, already split and stripped of field codes.
    Command { argv: Vec<String>, dir: Option<PathBuf> },
    Manual(manual::ManualGame),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    /// `steam:570`, `lutris:12`, `heroic:legendary:Fortnite`, `desktop:org.foo`,
    /// `manual:my-game`.
    pub id: String,
    pub name: String,
    pub source: Source,
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    /// Remote cover to fetch when there's no local one (Heroic art).
    pub cover_url: Option<String>,
    pub last_played: Option<i64>,
    pub playtime_minutes: Option<u64>,
    pub installed: bool,
    pub install_dir: Option<PathBuf>,
    pub launch: Launch,
    /// App id of the non-Steam shortcut we added to Steam for this game
    /// (Steam Input), if any.
    pub shortcut_appid: Option<u32>,
}

impl Game {
    pub fn steam_appid(&self) -> Option<&str> {
        match &self.launch {
            Launch::Steam { appid } => Some(appid),
            _ => None,
        }
    }

    /// Window class the game's window is expected to have, when known.
    pub fn class_hint(&self) -> Option<String> {
        match &self.launch {
            Launch::Steam { appid } => Some(format!("steam_app_{appid}")),
            _ => None,
        }
    }
}

/// The JSON shape of `m3-games list --json` and `~/.cache/m3-games/library.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Export {
    pub id: String,
    pub name: String,
    pub source: Source,
    pub cover: Option<String>,
    pub hero: Option<String>,
    pub last_played: Option<i64>,
    pub playtime_minutes: Option<u64>,
    pub installed: bool,
    /// Present when the game has a Steam shortcut (Steam Input).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_shortcut_appid: Option<u32>,
    /// The Steam app id, for Steam games.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_appid: Option<String>,
}

impl From<&Game> for Export {
    fn from(g: &Game) -> Self {
        let abs = |p: &Option<PathBuf>| p.as_ref().filter(|p| p.is_absolute()).map(|p| p.to_string_lossy().into_owned());
        Export {
            id: g.id.clone(),
            name: g.name.clone(),
            source: g.source,
            cover: abs(&g.cover),
            hero: abs(&g.hero),
            last_played: g.last_played,
            playtime_minutes: g.playtime_minutes,
            installed: g.installed,
            steam_shortcut_appid: g.shortcut_appid,
            steam_appid: g.steam_appid().map(str::to_string),
        }
    }
}

/// Play time and last-played for games launched through m3-games, kept in
/// `~/.local/share/m3-games/stats.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stat {
    pub last_played: Option<i64>,
    pub playtime_minutes: u64,
}

pub type Stats = HashMap<String, Stat>;

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| crate::sys::paths::home().join(".local/share"))
        .join("m3-games")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| crate::sys::paths::home().join(".cache"))
        .join("m3-games")
}

pub fn stats_path() -> PathBuf {
    data_dir().join("stats.json")
}

pub fn load_stats() -> Stats {
    std::fs::read_to_string(stats_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_stats(stats: &Stats) -> std::io::Result<()> {
    write_atomic(&stats_path(), &serde_json::to_string_pretty(stats).unwrap_or_default())
}

/// Record a play session.
pub fn record_play(id: &str, started: i64, minutes: u64) {
    let mut stats = load_stats();
    let s = stats.entry(id.to_string()).or_default();
    s.last_played = Some(s.last_played.unwrap_or(0).max(started));
    s.playtime_minutes += minutes;
    let _ = save_stats(&stats);
}

pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(tmp, path)
}

/// Fold our own statistics into what the sources reported. Sources that
/// track play time themselves (Steam, Lutris) win for play time; the latest
/// last-played wins.
pub fn merge_stats(games: &mut [Game], stats: &Stats) {
    for g in games {
        if let Some(s) = stats.get(&g.id) {
            g.last_played = match (g.last_played, s.last_played) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
            if g.playtime_minutes.unwrap_or(0) == 0 && s.playtime_minutes > 0 {
                g.playtime_minutes = Some(s.playtime_minutes);
            }
        }
    }
}

/// Most recently played first, then by name.
pub fn sort_recent(games: &mut [Game]) {
    games.sort_by(|a, b| {
        b.last_played
            .unwrap_or(0)
            .cmp(&a.last_played.unwrap_or(0))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Which sources to scan.
#[derive(Clone, Debug, Default)]
pub struct Enabled {
    pub steam: bool,
    pub lutris: bool,
    pub heroic: bool,
    pub desktop: bool,
    /// Web API key and SteamID64, to list owned games that aren't installed.
    pub steam_owned: Option<(String, String)>,
}

/// Scan every enabled source. Never fails; unreadable sources are skipped.
pub fn scan(home: &Path, enabled: Enabled) -> Vec<Game> {
    let mut games = Vec::new();
    if enabled.steam {
        games.extend(steam::scan(home));
    }
    if enabled.lutris {
        games.extend(lutris::scan(home));
    }
    if enabled.heroic {
        games.extend(heroic::scan(home));
    }
    if enabled.desktop {
        games.extend(desktop::scan(home));
    }
    games.extend(manual::load().into_iter().map(manual::ManualGame::into_game));
    if enabled.steam_owned.is_some() {
        // Owned-but-not-installed Steam games, below the installed ones.
        if let Some((key, steamid)) = &enabled.steam_owned {
            if let Ok(owned) = crate::steam::owned::fetch(key, steamid) {
                games.extend(owned);
            }
        }
    }
    // Same id from two places (e.g. native + flatpak Steam): keep the first.
    let mut seen = std::collections::HashSet::new();
    games.retain(|g| seen.insert(g.id.clone()));
    let shortcuts = crate::steam::shortcuts::current(home);
    for g in &mut games {
        g.shortcut_appid = shortcuts.get(&g.id).copied();
    }
    merge_stats(&mut games, &load_stats());
    sort_recent(&mut games);
    games
}

pub fn export_json(games: &[Game]) -> String {
    let list: Vec<Export> = games.iter().map(Export::from).collect();
    serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into())
}

pub fn library_cache_path() -> PathBuf {
    cache_dir().join("library.json")
}

/// Rewrite `~/.cache/m3-games/library.json` for the shell.
pub fn write_cache(games: &[Game]) {
    let _ = write_atomic(&library_cache_path(), &export_json(games));
}

/// Directories worth watching for library changes.
pub fn watch_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = steam::roots(home).into_iter().map(|r| r.join("steamapps")).collect();
    dirs.extend(lutris::roots(home));
    dirs.push(home.join(".config/heroic"));
    dirs.push(home.join(".local/share/applications"));
    dirs.push(manual::path().parent().map(Path::to_path_buf).unwrap_or_default());
    dirs.retain(|d| d.is_dir());
    dirs
}

/// Lowercase, ASCII letters/digits and dashes, for folder names and ids.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "game".into()
    } else {
        out
    }
}

#[cfg(test)]
pub(crate) fn test_game(id: &str, name: &str, last: Option<i64>) -> Game {
    Game {
        id: id.into(),
        name: name.into(),
        source: Source::Manual,
        cover: None,
        hero: None,
        cover_url: None,
        last_played: last,
        playtime_minutes: None,
        installed: true,
        shortcut_appid: None,
        install_dir: None,
        launch: Launch::Command { argv: vec!["true".into()], dir: None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_shape() {
        let mut g = test_game("steam:570", "Dota 2", Some(1_700_000_000));
        g.source = Source::Steam;
        g.cover = Some(PathBuf::from("/tmp/c.jpg"));
        g.hero = Some(PathBuf::from("relative.jpg"));
        g.playtime_minutes = Some(90);
        let json = export_json(&[g]);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let o = &v[0];
        assert_eq!(o["id"], "steam:570");
        assert_eq!(o["source"], "steam");
        assert_eq!(o["cover"], "/tmp/c.jpg");
        assert!(o["hero"].is_null());
        assert_eq!(o["last_played"], 1_700_000_000);
        assert_eq!(o["playtime_minutes"], 90);
        assert_eq!(o["installed"], true);
        let keys: Vec<_> = o.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["id", "name", "source", "cover", "hero", "last_played", "playtime_minutes", "installed"]);
    }

    #[test]
    fn sorting_and_merging() {
        let mut games = vec![
            test_game("a", "Beta", None),
            test_game("b", "alpha", None),
            test_game("c", "Gamma", Some(10)),
        ];
        let mut stats = Stats::new();
        stats.insert("a".into(), Stat { last_played: Some(20), playtime_minutes: 5 });
        merge_stats(&mut games, &stats);
        sort_recent(&mut games);
        let ids: Vec<_> = games.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["a", "c", "b"]);
        assert_eq!(games[0].playtime_minutes, Some(5));
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("The Witcher 3: Wild Hunt"), "the-witcher-3-wild-hunt");
        assert_eq!(slugify("???"), "game");
    }
}
