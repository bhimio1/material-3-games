//! Per-game Steam settings stored in Steam's own text files: the
//! compatibility tool (`config/config.vdf`) and launch options
//! (`userdata/<id>/config/localconfig.vdf`). Steam must be closed when
//! these are written; see [`super::account::with_steam_closed`].

use super::textedit;
use std::path::{Path, PathBuf};

const COMPAT: [&str; 5] = ["InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping"];
const APPS: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];

#[derive(Clone, Debug, PartialEq)]
pub struct CompatTool {
    /// The internal name Steam stores (`proton_9`, `GE-Proton9-20`).
    pub name: String,
    pub display: String,
}

fn compat_tool_vdf_name(dir: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(dir.join("compatibilitytool.vdf")).ok()?;
    let doc = crate::library::vdf::parse(&text);
    let tools = doc.path(&["compatibilitytools", "compat_tools"])?;
    let (name, t) = tools.entries().first()?;
    Some((name.clone(), t.get_str("display_name").unwrap_or(name).to_string()))
}

/// Steam's own Proton builds use fixed internal names.
pub fn valve_proton_name(folder: &str) -> Option<String> {
    let f = folder.trim();
    if f == "Proton - Experimental" {
        return Some("proton_experimental".into());
    }
    if f == "Proton Hotfix" {
        return Some("proton_hotfix".into());
    }
    let v = f.strip_prefix("Proton ")?;
    let mut parts = v.split('.');
    let major = parts.next()?.trim();
    let minor = parts.next().unwrap_or("0").trim();
    if !major.chars().all(|c| c.is_ascii_digit()) || major.is_empty() {
        return None;
    }
    // Proton 5.0+ is proton_<major>; older ones include the minor.
    let n: u32 = major.parse().ok()?;
    Some(if n >= 5 { format!("proton_{n}") } else { format!("proton_{major}{minor}") })
}

/// System-wide compatibility tool folders (distribution packages such as
/// CachyOS's `proton-cachyos` install here).
pub const SYSTEM_TOOL_DIRS: [&str; 2] =
    ["/usr/share/steam/compatibilitytools.d", "/usr/local/share/steam/compatibilitytools.d"];

/// A friendlier label for tools whose `display_name` is a bare package
/// name: `proton-cachyos` → "Proton-CachyOS", `proton-cachyos-slr` →
/// "Proton-CachyOS (SLR)".
pub fn friendly_tool_name(name: &str, display: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("proton-cachyos") {
        return if lower.contains("slr") {
            "Proton-CachyOS (SLR)".into()
        } else {
            "Proton-CachyOS".into()
        };
    }
    display.to_string()
}

/// A tool in a compatibilitytools.d folder. Packages without a
/// compatibilitytool.vdf fall back to the folder name, which Steam
/// also uses as the internal name.
fn custom_tool(dir: &Path) -> Option<CompatTool> {
    let (name, display) = match compat_tool_vdf_name(dir) {
        Some(v) => v,
        None if dir.join("proton").is_file() => {
            let f = dir.file_name()?.to_string_lossy().into_owned();
            (f.clone(), f)
        }
        None => return None,
    };
    let display = friendly_tool_name(&name, &display);
    Some(CompatTool { name, display })
}

/// Installed compatibility tools: Valve's Proton builds in steamapps and
/// custom ones (GE-Proton, Proton-CachyOS, …) in compatibilitytools.d.
pub fn list_tools(root: &Path) -> Vec<CompatTool> {
    let system: Vec<PathBuf> = SYSTEM_TOOL_DIRS.iter().map(PathBuf::from).collect();
    list_tools_in(root, &system)
}

pub fn list_tools_in(root: &Path, system_dirs: &[PathBuf]) -> Vec<CompatTool> {
    let mut out: Vec<CompatTool> = Vec::new();
    for lib in crate::library::steam::library_folders(root) {
        if let Ok(rd) = std::fs::read_dir(lib.join("steamapps/common")) {
            for e in rd.flatten() {
                let folder = e.file_name().to_string_lossy().into_owned();
                if let Some(name) = valve_proton_name(&folder) {
                    out.push(CompatTool { name, display: folder });
                }
            }
        }
    }
    let mut custom = vec![root.join("compatibilitytools.d")];
    custom.extend(system_dirs.iter().cloned());
    for dir in custom {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                out.extend(custom_tool(&e.path()));
            }
        }
    }
    out.sort_by(|a, b| b.display.cmp(&a.display));
    // Sorting by label can separate duplicates; keep the first of each name.
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(t.name.clone()));
    out
}

/// `PROTONPATH` for umu-launched games when the user hasn't picked one:
/// the distribution's Proton-CachyOS when installed, else GE-Proton
/// (which umu downloads on demand).
pub fn default_proton() -> String {
    let system: Vec<PathBuf> = SYSTEM_TOOL_DIRS.iter().map(PathBuf::from).collect();
    default_proton_in(&system)
}

pub fn default_proton_in(system_dirs: &[PathBuf]) -> String {
    for dir in system_dirs {
        let p = dir.join("proton-cachyos");
        if p.join("proton").is_file() {
            return p.to_string_lossy().into_owned();
        }
    }
    "GE-Proton".into()
}

pub fn config_vdf(root: &Path) -> PathBuf {
    root.join("config/config.vdf")
}

pub fn localconfig(user_dir: &Path) -> PathBuf {
    user_dir.join("config/localconfig.vdf")
}

fn compat_path(appid: &str) -> Vec<&str> {
    let mut p: Vec<&str> = COMPAT.to_vec();
    p.push(appid);
    p.push("name");
    p
}

fn launch_path(appid: &str) -> Vec<&str> {
    let mut p: Vec<&str> = APPS.to_vec();
    p.push(appid);
    p.push("LaunchOptions");
    p
}

pub fn get_compat(text: &str, appid: &str) -> Option<String> {
    textedit::get(text, &compat_path(appid)).filter(|s| !s.is_empty())
}

pub fn set_compat(text: &str, appid: &str, tool: &str) -> String {
    let t = textedit::set(text, &compat_path(appid), tool);
    let mut cfg = COMPAT.to_vec();
    cfg.push(appid);
    cfg.push("config");
    let t = if textedit::get(&t, &cfg).is_none() { textedit::set(&t, &cfg, "") } else { t };
    let mut prio = COMPAT.to_vec();
    prio.push(appid);
    prio.push("priority");
    if textedit::get(&t, &prio).is_none() {
        textedit::set(&t, &prio, "250")
    } else {
        t
    }
}

pub fn get_launch_options(text: &str, appid: &str) -> String {
    textedit::get(text, &launch_path(appid)).unwrap_or_default()
}

pub fn set_launch_options(text: &str, appid: &str, options: &str) -> String {
    textedit::set(text, &launch_path(appid), options)
}

/// Helpers for the launch-options editor: wrappers go before `%command%`,
/// variables at the front.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Wrapper(&'static str),
    Env(&'static str),
}

pub const TOGGLES: &[(&str, Toggle)] = &[
    ("GameMode", Toggle::Wrapper("gamemoderun")),
    ("MangoHud", Toggle::Wrapper("mangohud")),
    ("Wayland (Proton)", Toggle::Env("PROTON_ENABLE_WAYLAND=1")),
    ("HDR (DXVK)", Toggle::Env("DXVK_HDR=1")),
    ("NTSync (Proton)", Toggle::Env("PROTON_USE_NTSYNC=1")),
    ("FSR upscaling (Proton GE)", Toggle::Env("WINE_FULLSCREEN_FSR=1")),
    ("Show FPS (DXVK HUD)", Toggle::Env("DXVK_HUD=fps")),
    ("CachyOS performance profile", Toggle::Wrapper(GAME_PERFORMANCE)),
];

/// CachyOS's wrapper (cachyos-settings) that switches the power profile
/// to performance while the game runs.
pub const GAME_PERFORMANCE: &str = "game-performance";

/// Toggles that make sense on this system: the CachyOS wrapper only when
/// it's installed.
pub fn available_toggles() -> impl Iterator<Item = &'static (&'static str, Toggle)> {
    let perf = crate::sys::cmd::exists(GAME_PERFORMANCE);
    TOGGLES.iter().filter(move |(_, t)| perf || *t != Toggle::Wrapper(GAME_PERFORMANCE))
}

fn split_command(opts: &str) -> (Vec<String>, Vec<String>) {
    match opts.split_once("%command%") {
        Some((before, after)) => (
            before.split_whitespace().map(str::to_string).collect(),
            after.split_whitespace().map(str::to_string).collect(),
        ),
        None => (Vec::new(), opts.split_whitespace().map(str::to_string).collect()),
    }
}

pub fn has_toggle(opts: &str, t: Toggle) -> bool {
    let (before, _) = split_command(opts);
    match t {
        Toggle::Wrapper(w) => before.iter().any(|x| x == w),
        Toggle::Env(e) => before.iter().any(|x| x == e),
    }
}

/// Turn a helper on or off. Options without `%command%` are game
/// arguments, so `%command%` is added in front of them when needed.
pub fn set_toggle(opts: &str, t: Toggle, on: bool) -> String {
    let (mut before, after) = split_command(opts);
    let word = match t {
        Toggle::Wrapper(w) | Toggle::Env(w) => w.to_string(),
    };
    before.retain(|x| *x != word);
    if on {
        match t {
            Toggle::Env(_) => before.insert(0, word),
            Toggle::Wrapper(_) => {
                // After variables, before other wrappers.
                let at = before.iter().position(|x| !x.contains('=')).unwrap_or(before.len());
                before.insert(at, word);
            }
        }
    }
    if before.is_empty() && after.is_empty() {
        return String::new();
    }
    let mut parts = before;
    parts.push("%command%".into());
    parts.extend(after);
    let joined = parts.join(" ");
    if joined == "%command%" {
        String::new()
    } else {
        joined
    }
}

/// Screenshots Steam took for this app, newest first.
pub fn screenshots(user_dir: &Path, appid: &str) -> Vec<PathBuf> {
    let dir = user_dir.join("760/remote").join(appid).join("screenshots");
    let mut shots: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "jpg" | "jpeg" | "png")))
                .collect()
        })
        .unwrap_or_default();
    shots.sort();
    shots.reverse();
    shots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proton_names() {
        assert_eq!(valve_proton_name("Proton 9.0 (Beta)").as_deref(), Some("proton_9"));
        assert_eq!(valve_proton_name("Proton 8.0").as_deref(), Some("proton_8"));
        assert_eq!(valve_proton_name("Proton 4.11").as_deref(), Some("proton_411"));
        assert_eq!(valve_proton_name("Proton - Experimental").as_deref(), Some("proton_experimental"));
        assert_eq!(valve_proton_name("Proton EasyAntiCheat Runtime"), None);
        assert_eq!(valve_proton_name("Portal 2"), None);
    }

    #[test]
    fn tools_listing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("steamapps/common/Proton 9.0 (Beta)")).unwrap();
        let ge = root.path().join("compatibilitytools.d/GE-Proton9-20");
        std::fs::create_dir_all(&ge).unwrap();
        std::fs::write(ge.join("compatibilitytool.vdf"), r#""compatibilitytools" { "compat_tools" { "GE-Proton9-20" { "install_path" "." "display_name" "GE-Proton9-20" "from_oslist" "windows" "to_oslist" "linux" } } }"#).unwrap();
        let tools = list_tools(root.path());
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"proton_9"));
        assert!(names.contains(&"GE-Proton9-20"));
    }

    #[test]
    fn system_tools_listing() {
        let root = tempfile::tempdir().unwrap();
        let sys = tempfile::tempdir().unwrap();
        for (folder, display) in [("proton-cachyos", "proton-cachyos-10.0-20250714"), ("proton-cachyos-slr", "proton-cachyos-10.0-20250714-slr")] {
            let d = sys.path().join(folder);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("proton"), "").unwrap();
            std::fs::write(
                d.join("compatibilitytool.vdf"),
                format!(r#""compatibilitytools" {{ "compat_tools" {{ "{folder}" {{ "install_path" "." "display_name" "{display}" }} }} }}"#),
            )
            .unwrap();
        }
        // A packaged tool without a vdf is still found by its folder.
        let bare = sys.path().join("proton-ge-custom");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("proton"), "").unwrap();
        std::fs::create_dir_all(sys.path().join("not-a-tool")).unwrap();
        let tools = list_tools_in(root.path(), &[sys.path().to_path_buf()]);
        let find = |n: &str| tools.iter().find(|t| t.name == n).map(|t| t.display.clone());
        assert_eq!(find("proton-cachyos").as_deref(), Some("Proton-CachyOS"));
        assert_eq!(find("proton-cachyos-slr").as_deref(), Some("Proton-CachyOS (SLR)"));
        assert_eq!(find("proton-ge-custom").as_deref(), Some("proton-ge-custom"));
        assert!(find("not-a-tool").is_none());
        // The same tool in two places is listed once.
        let twice = list_tools_in(root.path(), &[sys.path().to_path_buf(), sys.path().to_path_buf()]);
        assert_eq!(twice.iter().filter(|t| t.name == "proton-cachyos").count(), 1);
    }

    #[test]
    fn proton_default() {
        let sys = tempfile::tempdir().unwrap();
        assert_eq!(default_proton_in(&[sys.path().to_path_buf()]), "GE-Proton");
        let d = sys.path().join("proton-cachyos");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("proton"), "").unwrap();
        assert_eq!(default_proton_in(&[sys.path().to_path_buf()]), d.to_string_lossy());
    }

    #[test]
    fn compat_mapping() {
        let text = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"AutoUpdateWindowEnabled\"\t\t\"0\"\n\t\t\t}\n\t\t}\n\t}\n}\n";
        assert_eq!(get_compat(text, "570"), None);
        let t = set_compat(text, "570", "GE-Proton9-20");
        assert_eq!(get_compat(&t, "570").as_deref(), Some("GE-Proton9-20"));
        assert!(t.contains("\"AutoUpdateWindowEnabled\"\t\t\"0\""));
        let t2 = set_compat(&t, "570", "proton_experimental");
        assert_eq!(get_compat(&t2, "570").as_deref(), Some("proton_experimental"));
        assert_eq!(t2.matches("\"priority\"").count(), 1);
    }

    #[test]
    fn launch_option_toggles() {
        let gm = TOGGLES[0].1;
        let wl = TOGGLES[2].1;
        assert_eq!(set_toggle("", gm, true), "gamemoderun %command%");
        assert_eq!(set_toggle("-novid", gm, true), "gamemoderun %command% -novid");
        let o = set_toggle("gamemoderun %command% -novid", wl, true);
        assert_eq!(o, "PROTON_ENABLE_WAYLAND=1 gamemoderun %command% -novid");
        assert!(has_toggle(&o, wl) && has_toggle(&o, gm));
        assert_eq!(set_toggle(&o, gm, false), "PROTON_ENABLE_WAYLAND=1 %command% -novid");
        assert_eq!(set_toggle("gamemoderun %command%", gm, false), "");
        let lc = "\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"apps\" { } } } } }";
        let t = set_launch_options(lc, "570", "mangohud %command%");
        assert_eq!(get_launch_options(&t, "570"), "mangohud %command%");
    }

    #[test]
    fn screenshot_listing() {
        let user = tempfile::tempdir().unwrap();
        let d = user.path().join("760/remote/570/screenshots");
        std::fs::create_dir_all(d.join("thumbnails")).unwrap();
        std::fs::write(d.join("20240101_1.jpg"), b"x").unwrap();
        std::fs::write(d.join("20240202_1.jpg"), b"x").unwrap();
        let s = screenshots(user.path(), "570");
        assert_eq!(s.len(), 2);
        assert!(s[0].ends_with("20240202_1.jpg"));
    }
}
