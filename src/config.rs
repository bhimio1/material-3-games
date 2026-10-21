//! Preferences (`~/.config/m3-games/config.toml`): theme, sources, Hyprland
//! behaviour, gamescope defaults and per-game launch options.

use material_3_ui::theme::ThemeSettings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// This app's seed color when there's no desktop palette.
pub const DEFAULT_SEED: &str = "#7C4DFF";

/// The appearance settings as stored at the top of config.toml, under
/// this app's own names; in memory they're the shared [`ThemeSettings`].
mod stored_appearance {
    use super::DEFAULT_SEED;
    use material_3_ui::theme::{ColorScheme, ThemeSettings, ThemeSource};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Deserialize, Serialize)]
    #[serde(default)]
    struct Stored {
        theme_source: ThemeSource,
        color_scheme: ColorScheme,
        seed_color: String,
        filled_icons: bool,
        ui_scale: f32,
    }

    impl Default for Stored {
        fn default() -> Self {
            Self {
                theme_source: ThemeSource::Auto,
                color_scheme: ColorScheme::System,
                seed_color: DEFAULT_SEED.into(),
                filled_icons: true,
                ui_scale: 1.0,
            }
        }
    }

    pub fn default() -> ThemeSettings {
        let s = Stored::default();
        ThemeSettings {
            source: s.theme_source,
            scheme: s.color_scheme,
            seed_color: s.seed_color,
            filled_icons: s.filled_icons,
            ui_scale: s.ui_scale,
        }
    }

    pub fn serialize<S: Serializer>(a: &ThemeSettings, ser: S) -> Result<S::Ok, S::Error> {
        Stored {
            theme_source: a.source,
            color_scheme: a.scheme,
            seed_color: a.seed_color.clone(),
            filled_icons: a.filled_icons,
            ui_scale: a.ui_scale,
        }
        .serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<ThemeSettings, D::Error> {
        let s = Stored::deserialize(de)?;
        Ok(ThemeSettings {
            source: s.theme_source,
            scheme: s.color_scheme,
            seed_color: s.seed_color,
            filled_icons: s.filled_icons,
            ui_scale: s.ui_scale,
        })
    }
}

/// gamescope settings for Game Mode and for per-game desktop use.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Gamescope {
    /// 0 = the monitor's own.
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
    /// AMD FidelityFX Super Resolution upscaling (`-F fsr`).
    pub fsr: bool,
    pub hdr: bool,
    pub adaptive_sync: bool,
    /// Extra arguments, split on whitespace.
    pub extra: String,
}

impl Default for Gamescope {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            refresh: 0,
            fsr: false,
            hdr: false,
            adaptive_sync: true,
            extra: String::new(),
        }
    }
}

impl Gamescope {
    /// Arguments before `--`. `display` (w, h, hz) fills in zeros.
    pub fn args(&self, display: Option<(u32, u32, u32)>) -> Vec<String> {
        let (dw, dh, dr) = display.unwrap_or((0, 0, 0));
        let pick = |own: u32, d: u32| if own > 0 { own } else { d };
        let (w, h, r) = (pick(self.width, dw), pick(self.height, dh), pick(self.refresh, dr));
        let mut a = vec!["-f".to_string()];
        if w > 0 && h > 0 {
            a.extend(["-W".into(), w.to_string(), "-H".into(), h.to_string()]);
        }
        if r > 0 {
            a.extend(["-r".into(), r.to_string()]);
        }
        if self.adaptive_sync {
            a.push("--adaptive-sync".into());
        }
        if self.fsr {
            a.extend(["-F".into(), "fsr".into()]);
        }
        if self.hdr {
            a.push("--hdr-enabled".into());
        }
        a.extend(self.extra.split_whitespace().map(str::to_string));
        a
    }
}

/// Launch options for one game.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct GameOptions {
    pub gamemode: bool,
    /// CachyOS `game-performance` wrapper (performance power profile while
    /// playing). Overlaps with GameMode; both may be on.
    pub cachyos_perf: bool,
    pub mangohud: bool,
    /// Run inside gamescope on the desktop too.
    pub gamescope: bool,
    pub force_fullscreen: bool,
    /// `KEY=value` lines.
    pub env: Vec<String>,
    /// Launch through a Steam shortcut so Steam Input (and Steam's overlay)
    /// apply. Steam games always have it.
    pub steam_input: bool,
    /// Free-form notes, shown in the detail page and the Game Mode overlay.
    pub notes: String,
    /// Guides pinned for quick access.
    pub guides: Vec<PinnedGuide>,
    /// Game Mode overlay settings, re-applied when the game is focused.
    pub overlay: OverlayPrefs,
}

impl Default for GameOptions {
    fn default() -> Self {
        Self {
            gamemode: false,
            cachyos_perf: false,
            mangohud: false,
            gamescope: false,
            force_fullscreen: true,
            env: Vec::new(),
            steam_input: false,
            notes: String::new(),
            guides: Vec::new(),
            overlay: OverlayPrefs::default(),
        }
    }
}

/// A guide pinned to a game: a Steam guide id, a web page, or a local
/// Markdown/text file.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct PinnedGuide {
    pub title: String,
    /// Steam guide id (`sharedfiles/filedetails/?id=`).
    pub steam_id: String,
    /// URL or absolute path, for anything else.
    pub location: String,
}

/// Per-game gamescope settings from the Game Mode overlay.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct OverlayPrefs {
    /// 0 = no limit.
    pub fps_limit: u32,
    pub allow_tearing: bool,
    /// gamescope's GamescopeUpscaleFilter: 0 linear, 1 nearest, 2 FSR,
    /// 3 NIS, 4 pixel, 5 SGSR.
    pub filter: u32,
    /// 0 (sharpest) … 20.
    pub sharpness: u32,
    /// 0 off, 1 FPS only, 2 horizontal bar, 3 full.
    pub perf_overlay: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Stored flat as `theme_source`, `color_scheme`, `seed_color`,
    /// `filled_icons` and `ui_scale`.
    #[serde(flatten, with = "stored_appearance")]
    pub appearance: ThemeSettings,
    pub steam: bool,
    pub lutris: bool,
    pub heroic: bool,
    pub desktop_entries: bool,
    pub steamgriddb_key: String,
    /// Turn off animations, blur and shadows in Hyprland while a game runs.
    pub performance_mode: bool,
    pub gamescope: Gamescope,
    pub games: BTreeMap<String, GameOptions>,
    /// Window classes seen for games we launched, added to the static rules.
    pub learned_classes: Vec<String>,
    pub terminal: String,
    /// Steam Web API key (optional): exact achievements and owned games.
    pub steam_api_key: String,
    /// SteamID64 for the Web API; empty = the most recent local login.
    pub steam_id: String,
    /// List owned Steam games that aren't installed (needs the key).
    pub show_uninstalled: bool,
    /// Switch to a sched-ext scheduler (via scxctl) in Game Mode.
    pub scx_gaming: bool,
    /// Also while a game runs on the desktop.
    pub scx_desktop: bool,
    /// Scheduler name without `scx_` (`lavd`).
    pub scx_sched: String,
    /// scx_loader mode (`gaming`).
    pub scx_mode: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            appearance: stored_appearance::default(),
            steam: true,
            lutris: true,
            heroic: true,
            desktop_entries: true,
            steamgriddb_key: String::new(),
            performance_mode: true,
            gamescope: Gamescope::default(),
            games: BTreeMap::new(),
            learned_classes: Vec::new(),
            terminal: String::new(),
            steam_api_key: String::new(),
            steam_id: String::new(),
            show_uninstalled: false,
            scx_gaming: false,
            scx_desktop: false,
            scx_sched: "lavd".into(),
            scx_mode: "gaming".into(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        crate::sys::paths::config_home().join("m3-games/config.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::library::write_atomic(&Self::path(), &toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn options(&self, id: &str) -> GameOptions {
        self.games.get(id).cloned().unwrap_or_default()
    }

    pub fn enabled(&self) -> crate::library::Enabled {
        crate::library::Enabled {
            steam: self.steam,
            lutris: self.lutris,
            heroic: self.heroic,
            desktop: self.desktop_entries,
            steam_owned: (self.steam && self.show_uninstalled && !self.steam_api_key.is_empty())
                .then(|| (self.steam_api_key.clone(), self.steamid64()))
                .filter(|(_, id)| !id.is_empty()),
        }
    }

    /// Sources without anything that needs the network (for launching).
    pub fn enabled_local(&self) -> crate::library::Enabled {
        crate::library::Enabled { steam_owned: None, ..self.enabled() }
    }

    /// The configured SteamID64, else the most recent local login.
    pub fn steamid64(&self) -> String {
        if !self.steam_id.trim().is_empty() {
            return self.steam_id.trim().to_string();
        }
        crate::steam::account::root(&crate::sys::paths::home())
            .and_then(|r| crate::steam::account::most_recent_steamid(&r))
            .map(|id| id.to_string())
            .unwrap_or_default()
    }

    /// Remember a game's window class. Returns true if it was new.
    pub fn learn_class(&mut self, class: &str) -> bool {
        if class.is_empty() || self.learned_classes.iter().any(|c| c == class) {
            return false;
        }
        self.learned_classes.push(class.to_string());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamescope_args() {
        let g = Gamescope::default();
        assert_eq!(g.args(Some((2560, 1440, 165))), ["-f", "-W", "2560", "-H", "1440", "-r", "165", "--adaptive-sync"]);
        assert_eq!(g.args(None), ["-f", "--adaptive-sync"]);
        let g = Gamescope { width: 1920, height: 1080, fsr: true, adaptive_sync: false, extra: "--mangoapp".into(), ..Default::default() };
        assert_eq!(g.args(Some((2560, 1440, 60))), ["-f", "-W", "1920", "-H", "1080", "-r", "60", "-F", "fsr", "--mangoapp"]);
    }

    #[test]
    fn partial_config() {
        let c: Config = toml::from_str("performance_mode = false\n[games.\"steam:570\"]\nmangohud = true\n").unwrap();
        assert!(!c.performance_mode);
        let o = c.options("steam:570");
        assert!(o.mangohud && o.force_fullscreen);
        assert!(c.options("other").force_fullscreen);
        let mut c = c;
        assert!(c.learn_class("hl2_linux"));
        assert!(!c.learn_class("hl2_linux"));
        assert_eq!(c.appearance.seed_color, DEFAULT_SEED);
    }

    #[test]
    fn appearance_keeps_its_stored_names() {
        use material_3_ui::theme::{ColorScheme, ThemeSource};
        let old = "theme_source = \"seed\"\ncolor_scheme = \"light\"\nseed_color = \"#1E8E3E\"\nfilled_icons = false\nui_scale = 1.25\nperformance_mode = false\n\n[gamescope]\nfsr = true\n";
        let c: Config = toml::from_str(old).unwrap();
        assert_eq!(c.appearance.source, ThemeSource::Seed);
        assert_eq!(c.appearance.scheme, ColorScheme::Light);
        assert_eq!(c.appearance.seed_color, "#1E8E3E");
        assert!(!c.appearance.filled_icons);
        assert_eq!(c.appearance.ui_scale, 1.25);
        assert!(!c.performance_mode && c.gamescope.fsr);

        let saved = toml::to_string_pretty(&c).unwrap();
        assert!(saved.contains("theme_source = \"seed\""), "{saved}");
        assert!(saved.contains("color_scheme = \"light\""));
        assert!(!saved.contains("appearance"));
        let back: Config = toml::from_str(&saved).unwrap();
        assert_eq!(back, c);
    }
}
