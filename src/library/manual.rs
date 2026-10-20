//! Games added by hand, stored in `~/.config/m3-games/games.toml`. Windows
//! games run through umu-launcher (Proton) or Wine, each with its own prefix.

use super::{slugify, Game, Launch, Source};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runner {
    #[default]
    Native,
    /// umu-launcher with Proton.
    Umu,
    Wine,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManualGame {
    pub slug: String,
    pub name: String,
    pub exe: String,
    pub args: String,
    pub dir: String,
    /// `KEY=value` lines.
    pub env: Vec<String>,
    pub runner: Runner,
    /// PROTONPATH for umu (`GE-Proton` = latest GE-Proton, downloaded by umu;
    /// empty = Proton-CachyOS when installed, else GE-Proton).
    pub proton: String,
    pub cover: String,
}

impl ManualGame {
    #[cfg(test)]
    pub fn new(name: &str, exe: &str) -> Self {
        let runner = if exe.to_lowercase().ends_with(".exe") || exe.to_lowercase().ends_with(".msi") {
            Runner::Umu
        } else {
            Runner::Native
        };
        Self {
            slug: slugify(name),
            name: name.to_string(),
            exe: exe.to_string(),
            runner,
            ..Default::default()
        }
    }

    pub fn id(&self) -> String {
        format!("manual:{}", self.slug)
    }

    /// Per-game Wine prefix.
    pub fn prefix(&self) -> PathBuf {
        crate::sys::paths::home().join("Games/m3-games/prefixes").join(&self.slug)
    }

    pub fn into_game(self) -> Game {
        let cover = Some(PathBuf::from(&self.cover)).filter(|p| p.is_absolute() && p.is_file());
        let exe = PathBuf::from(&self.exe);
        Game {
            id: self.id(),
            name: self.name.clone(),
            source: Source::Manual,
            cover,
            hero: None,
            cover_url: None,
            last_played: None,
            playtime_minutes: None,
            installed: exe.is_file() || crate::sys::cmd::exists(&self.exe),
            shortcut_appid: None,
            install_dir: exe.parent().map(PathBuf::from).filter(|p| p.is_dir()),
            launch: Launch::Manual(self),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default, rename = "game")]
    games: Vec<ManualGame>,
}

pub fn path() -> PathBuf {
    crate::sys::paths::config_home().join("m3-games/games.toml")
}

pub fn load() -> Vec<ManualGame> {
    parse(&std::fs::read_to_string(path()).unwrap_or_default())
}

pub fn parse(text: &str) -> Vec<ManualGame> {
    toml::from_str::<File>(text).map(|f| f.games).unwrap_or_default()
}

pub fn render(games: &[ManualGame]) -> String {
    toml::to_string_pretty(&File { games: games.to_vec() }).unwrap_or_default()
}

pub fn save(games: &[ManualGame]) -> std::io::Result<()> {
    super::write_atomic(&path(), &render(games))
}

/// Add or replace (by slug) a game. A new game whose slug is taken gets a
/// numbered one.
pub fn upsert(mut game: ManualGame, replacing: Option<&str>) -> std::io::Result<String> {
    let mut games = load();
    if let Some(old) = replacing {
        games.retain(|g| g.slug != old);
    }
    let base = if game.slug.is_empty() { slugify(&game.name) } else { game.slug.clone() };
    let mut slug = base.clone();
    let mut n = 2;
    while games.iter().any(|g| g.slug == slug) {
        slug = format!("{base}-{n}");
        n += 1;
    }
    game.slug = slug.clone();
    games.push(game);
    save(&games)?;
    Ok(slug)
}

pub fn remove(slug: &str) -> std::io::Result<()> {
    let mut games = load();
    games.retain(|g| g.slug != slug);
    save(&games)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut g = ManualGame::new("My Game", "/games/my/game.exe");
        g.env = vec!["DXVK_HUD=1".into()];
        assert_eq!(g.runner, Runner::Umu);
        assert_eq!(g.id(), "manual:my-game");
        let text = render(&[g.clone()]);
        assert_eq!(parse(&text), vec![g]);
        assert!(parse("not = [toml").is_empty());
        assert_eq!(ManualGame::new("x", "/usr/bin/x").runner, Runner::Native);
    }
}
