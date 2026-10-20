//! Controlling gamescope from inside its session through the X11
//! properties it watches (see gamescope's steamcompmgr.cpp):
//!
//! * root `GAMESCOPECTRL_BASELAYER_APPID`: app ids in focus order; the first
//!   one with a window gets focus.
//! * window `STEAM_GAME`: a window's app id (authoritative in `-e` mode).
//! * window `STEAM_OVERLAY` = 1: drawn above the game, blended by
//!   `_NET_WM_WINDOW_OPACITY`; `STEAM_INPUT_FOCUS` = 1 gives it mouse and
//!   keyboard (2 would leave the keyboard with the game).
//! * root `GAMESCOPE_FPS_LIMIT`, `GAMESCOPE_ALLOW_TEARING`,
//!   `GAMESCOPE_NEW_SCALING_FILTER`, `GAMESCOPE_SHARPNESS`,
//!   `GAMESCOPECTRL_REQUEST_SCREENSHOT`.
//!
//! In `-e` mode gamescope gives a game's windows the app id found in its
//! cgroup scope name `app-steam-app<ID>-<N>.scope`, so non-Steam games are
//! started in such a scope with `systemd-run --user --scope`.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// App id of the big-picture UI window (distinct from Steam's 769).
pub const UI_APPID: u32 = 7_690_001;
/// App id of the overlay window.
pub const OVERLAY_APPID: u32 = 7_690_002;
/// `_NET_WM_WINDOW_OPACITY` value for fully opaque.
pub const OPAQUE: u32 = 0xFFFF_FFFF;

/// gamescope's screenshot types (gamescope-control.xml).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // mirrors the protocol; the overlay uses AllRealLayers
pub enum ScreenshotType {
    BasePlaneOnly = 1,
    AllRealLayers = 2,
    FullComposition = 3,
    ScreenBuffer = 4,
}

/// Where gamescope writes screenshots requested through the X property.
pub const SCREENSHOT_PATH: &str = "/tmp/gamescope.png";

/// Upscaling filters (GamescopeUpscaleFilter in main.hpp).
pub const FILTERS: [(u32, &str); 6] = [(0, "Linear"), (1, "Nearest"), (2, "FSR"), (3, "NIS"), (4, "Pixel"), (5, "SGSR")];

/// The gamescope app id for a game: Steam games keep theirs, others get a
/// stable hash that avoids 0, Steam's 769 and our own ids.
pub fn appid_for(game_id: &str, steam_appid: Option<&str>) -> u32 {
    if let Some(id) = steam_appid.and_then(|a| a.parse::<u32>().ok()).filter(|a| *a != 0) {
        return id;
    }
    let mut h = crc32fast::Hasher::new();
    h.update(game_id.as_bytes());
    let mut id = h.finalize() & 0x7FFF_FFFF;
    if matches!(id, 0 | 769 | UI_APPID | OVERLAY_APPID) {
        id = id.wrapping_add(1_000_003) & 0x7FFF_FFFF;
    }
    id
}

/// systemd scope unit name matching gamescope's
/// `sscanf(scope, "app-steam-app%u-%d.scope", &appid, &reaperpid)`.
pub fn scope_unit(appid: u32, n: u32) -> String {
    format!("app-steam-app{appid}-{n}.scope")
}

/// Mirror of gamescope's parser, for tests.
#[cfg(test)]
pub fn parse_scope(scope: &str) -> Option<u32> {
    let rest = scope.strip_prefix("app-steam-app")?;
    let (id, rest) = rest.split_once('-')?;
    let (n, tail) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len()));
    let appid: u32 = id.parse().ok()?;
    let _: i32 = n.parse().ok()?;
    (tail.starts_with(".scope") && appid != 0).then_some(appid)
}

/// Wrap `argv` so the game runs in a scope gamescope maps to `appid`.
/// Without systemd-run the command is returned unchanged (gamescope then
/// can't tell the game apart; focus falls back to the newest window).
pub fn wrap_in_scope(argv: Vec<String>, appid: u32) -> Vec<String> {
    if !crate::sys::cmd::exists("systemd-run") || argv.is_empty() {
        return argv;
    }
    // A unique number in the pid slot; gamescope only checks it parses.
    let n = std::process::id().wrapping_mul(31).wrapping_add(crate::launch::now() as u32) & 0x7FFF_FFFF;
    let mut out = vec![
        "systemd-run".to_string(),
        "--user".into(),
        "--scope".into(),
        "--quiet".into(),
        "--collect".into(),
        format!("--unit={}", scope_unit(appid, n)),
        "--".into(),
    ];
    out.extend(argv);
    out
}

/// Focus order for `GAMESCOPECTRL_BASELAYER_APPID`.
pub fn baselayer(game: Option<u32>, show_ui: bool) -> Vec<u32> {
    match (game, show_ui) {
        (Some(g), false) => vec![g, UI_APPID],
        (Some(g), true) => vec![UI_APPID, g],
        (None, _) => vec![UI_APPID],
    }
}

/// An X connection to gamescope's Xwayland (`$DISPLAY`).
pub struct Ctl {
    conn: RustConnection,
    pub root: u32,
}

impl Ctl {
    pub fn connect() -> Result<Ctl, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots.get(screen).ok_or("no X screen")?.root;
        Ok(Ctl { conn, root })
    }

    pub fn atom(&self, name: &str) -> Result<u32, String> {
        self.conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| e.to_string())?
            .reply()
            .map(|r| r.atom)
            .map_err(|e| e.to_string())
    }

    /// Set a CARDINAL (list) property on `window`.
    pub fn set(&self, window: u32, name: &str, values: &[u32]) -> Result<(), String> {
        let atom = self.atom(name)?;
        self.conn
            .change_property32(PropMode::REPLACE, window, atom, AtomEnum::CARDINAL, values)
            .map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())
    }

    #[cfg(test)]
    pub fn delete(&self, window: u32, name: &str) -> Result<(), String> {
        let atom = self.atom(name)?;
        self.conn.delete_property(window, atom).map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())
    }

    #[cfg(test)]
    pub fn get(&self, window: u32, name: &str) -> Option<Vec<u32>> {
        let atom = self.atom(name).ok()?;
        let reply = self.conn.get_property(false, window, atom, AtomEnum::CARDINAL, 0, 64).ok()?.reply().ok()?;
        let v: Vec<u32> = reply.value32()?.collect();
        (!v.is_empty()).then_some(v)
    }

    pub fn set_root(&self, name: &str, values: &[u32]) -> Result<(), String> {
        self.set(self.root, name, values)
    }

    /// Who gets focus, in order.
    pub fn focus(&self, order: &[u32]) -> Result<(), String> {
        self.set_root("GAMESCOPECTRL_BASELAYER_APPID", order)
    }

    pub fn fps_limit(&self, fps: u32) -> Result<(), String> {
        self.set_root("GAMESCOPE_FPS_LIMIT", &[fps])
    }

    pub fn allow_tearing(&self, on: bool) -> Result<(), String> {
        self.set_root("GAMESCOPE_ALLOW_TEARING", &[on as u32])
    }

    pub fn scaling(&self, filter: u32, sharpness: u32) -> Result<(), String> {
        self.set_root("GAMESCOPE_NEW_SCALING_FILTER", &[filter.min(5)])?;
        self.set_root("GAMESCOPE_SHARPNESS", &[sharpness.min(20)])
    }

    pub fn screenshot(&self, kind: ScreenshotType) -> Result<(), String> {
        self.set_root("GAMESCOPECTRL_REQUEST_SCREENSHOT", &[kind as u32])
    }

    /// Tag a window with an app id.
    pub fn tag_window(&self, window: u32, appid: u32) -> Result<(), String> {
        self.set(window, "STEAM_GAME", &[appid])
    }

    /// Show or hide an overlay window.
    pub fn overlay(&self, window: u32, shown: bool) -> Result<(), String> {
        self.set(window, "STEAM_OVERLAY", &[1])?;
        self.set(window, "_NET_WM_WINDOW_OPACITY", &[if shown { OPAQUE } else { 0 }])?;
        self.set(window, "STEAM_INPUT_FOCUS", &[shown as u32])
    }
}

/// The X11 window id of an eframe window, when it is an X11 window.
pub fn x11_window(handle: &impl raw_window_handle::HasWindowHandle) -> Option<u32> {
    match handle.window_handle().ok()?.as_raw() {
        raw_window_handle::RawWindowHandle::Xlib(h) => Some(h.window as u32),
        raw_window_handle::RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

/// gamescope with Steam integration is our session (Game Mode).
pub fn active() -> bool {
    std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some()
}

/// MangoHud configuration for the performance overlay levels, written to
/// `$MANGOHUD_CONFIGFILE` (gamescope `--mangoapp` re-reads it on change).
pub fn mangohud_config(level: u32) -> &'static str {
    match level {
        0 => "no_display\n",
        1 => "fps_only\nfps\nbackground_alpha=0.4\nposition=top-left\n",
        2 => "horizontal\nlegacy_layout=0\ntable_columns=20\nfps\nframetime=0\ncpu_stats\ngpu_stats\nram\nvram\nbattery\nhud_no_margin\nposition=top-center\nbackground_alpha=0.4\n",
        _ => "full\nposition=top-left\nbackground_alpha=0.5\n",
    }
}

pub fn mangohud_config_path() -> std::path::PathBuf {
    crate::session::runtime_dir().join("mangohud.conf")
}

pub fn set_perf_overlay(level: u32) -> Result<(), String> {
    let path = std::env::var_os("MANGOHUD_CONFIGFILE").map(std::path::PathBuf::from).unwrap_or_else(mangohud_config_path);
    crate::library::write_atomic(&path, mangohud_config(level)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_names_match_gamescope() {
        let u = scope_unit(620, 12345);
        assert_eq!(u, "app-steam-app620-12345.scope");
        assert_eq!(parse_scope(&u), Some(620));
        assert_eq!(parse_scope("app-steam-app0-1.scope"), None);
        assert_eq!(parse_scope("app-foo.scope"), None);
        let wrapped = wrap_in_scope(vec!["true".into()], 7);
        if crate::sys::cmd::exists("systemd-run") {
            assert_eq!(&wrapped[..3], ["systemd-run", "--user", "--scope"]);
            let unit = wrapped.iter().find_map(|a| a.strip_prefix("--unit=")).unwrap();
            assert_eq!(parse_scope(unit), Some(7));
            assert_eq!(wrapped.last().map(String::as_str), Some("true"));
        } else {
            assert_eq!(wrapped, ["true"]);
        }
    }

    #[test]
    fn app_ids() {
        assert_eq!(appid_for("steam:620", Some("620")), 620);
        let a = appid_for("lutris:3", None);
        assert_eq!(a, appid_for("lutris:3", None));
        assert!(a != 0 && a != 769 && a < 0x8000_0000);
        assert_ne!(a, appid_for("lutris:4", None));
    }

    #[test]
    fn root_values() {
        assert_eq!(baselayer(Some(620), false), [620, UI_APPID]);
        assert_eq!(baselayer(Some(620), true), [UI_APPID, 620]);
        assert_eq!(baselayer(None, false), [UI_APPID]);
        assert!(mangohud_config(0).contains("no_display"));
    }

    /// Sets the properties on a real X server when one is available
    /// (`xvfb-run cargo test`), and reads them back.
    #[test]
    fn x11_properties_round_trip() {
        if std::env::var_os("DISPLAY").is_none() {
            return;
        }
        let Ok(ctl) = Ctl::connect() else {
            return;
        };
        ctl.focus(&baselayer(Some(620), false)).unwrap();
        assert_eq!(ctl.get(ctl.root, "GAMESCOPECTRL_BASELAYER_APPID"), Some(vec![620, UI_APPID]));
        ctl.fps_limit(40).unwrap();
        assert_eq!(ctl.get(ctl.root, "GAMESCOPE_FPS_LIMIT"), Some(vec![40]));
        ctl.scaling(2, 25).unwrap();
        assert_eq!(ctl.get(ctl.root, "GAMESCOPE_NEW_SCALING_FILTER"), Some(vec![2]));
        assert_eq!(ctl.get(ctl.root, "GAMESCOPE_SHARPNESS"), Some(vec![20]));
        ctl.overlay(ctl.root, true).unwrap();
        assert_eq!(ctl.get(ctl.root, "STEAM_INPUT_FOCUS"), Some(vec![1]));
        assert_eq!(ctl.get(ctl.root, "_NET_WM_WINDOW_OPACITY"), Some(vec![OPAQUE]));
        for p in ["GAMESCOPECTRL_BASELAYER_APPID", "GAMESCOPE_FPS_LIMIT", "GAMESCOPE_NEW_SCALING_FILTER", "GAMESCOPE_SHARPNESS", "STEAM_OVERLAY", "STEAM_INPUT_FOCUS", "_NET_WM_WINDOW_OPACITY"] {
            ctl.delete(ctl.root, p).unwrap();
        }
        assert_eq!(ctl.get(ctl.root, "GAMESCOPE_FPS_LIMIT"), None);
    }
}
