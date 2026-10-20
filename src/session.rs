//! Game Mode: leaving Hyprland for a bare gamescope session (when started
//! from the `m3-session` login session), or a nested gamescope fallback.
//!
//! Coordination with `m3-session` goes through files in
//! `$XDG_RUNTIME_DIR/m3-games/`:
//!   next            "game-mode" when gamescope should start after Hyprland exits
//!   gamescope-args  one gamescope argument per line
//!   error           why the last Game Mode start failed (shown as a toast)
//!   running.json    the game currently running, for `status --json`

use crate::config::Config;
use crate::hypr;
use crate::sys::cmd;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const SESSION_SCRIPT: &str = include_str!("../packaging/m3-session");
pub const SESSION_DESKTOP: &str = include_str!("../packaging/m3-games-session.desktop");
pub const SESSION_DESKTOP_PATH: &str = "/usr/share/wayland-sessions/m3-games-session.desktop";

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("m3-games")
}

/// Started from the `Hyprland (with Game Mode)` login session.
pub fn session_ready() -> bool {
    std::env::var("M3_SESSION").is_ok_and(|v| v == "1")
}

/// Running inside gamescope.
pub fn in_gamescope() -> bool {
    std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some()
}

/// The session entry is installed system-wide.
pub fn session_installed() -> bool {
    Path::new(SESSION_DESKTOP_PATH).is_file()
}

/// Write the flag and gamescope arguments `m3-session` reads after
/// Hyprland exits.
pub fn write_flag(dir: &Path, gamescope_args: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("gamescope-args"), gamescope_args.join("\n") + "\n")?;
    std::fs::write(dir.join("next"), "game-mode\n")
}

#[cfg(test)]
pub fn read_flag(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("next")).is_ok_and(|s| s.trim() == "game-mode")
}

/// Take the last Game Mode error, if any.
pub fn take_error() -> Option<String> {
    let path = runtime_dir().join("error");
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(path);
    Some(text.trim().to_string()).filter(|t| !t.is_empty())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
pub struct Running {
    pub id: String,
    pub name: String,
    pub pid: u32,
    /// Unix seconds the game was started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<i64>,
    /// gamescope app id (Game Mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gamescope_appid: Option<u32>,
    /// systemd scope the game runs in (Game Mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

pub fn write_running(id: &str, name: &str, pid: u32) {
    write_running_full(&Running { id: id.into(), name: name.into(), pid, started: Some(crate::launch::now()), ..Default::default() });
}

pub fn write_running_full(r: &Running) {
    let _ = crate::library::write_atomic(&runtime_dir().join("running.json"), &serde_json::to_string(r).unwrap_or_default());
}

pub fn clear_running(id: &str) {
    let path = runtime_dir().join("running.json");
    if read_running_at(&path).is_some_and(|r| r.id == id) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn read_running() -> Option<Running> {
    read_running_at(&runtime_dir().join("running.json"))
}

fn read_running_at(path: &Path) -> Option<Running> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The running game, if its process (or, for store launches, its window
/// watcher) is still around.
pub fn running_game() -> Option<Running> {
    let r = read_running_at(&runtime_dir().join("running.json"))?;
    (hypr::alive(r.pid) || watcher_alive(&r.id)).then_some(r)
}

/// Processes whose command line contains all of `needles`.
pub fn find_processes(needles: &[&str]) -> Vec<u32> {
    let me = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
        .filter(|pid| *pid != me)
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|raw| {
                let args: Vec<String> = raw.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
                args.first().is_some_and(|a| a.ends_with("m3-games")) && needles.iter().all(|n| args.iter().any(|a| a == n))
            })
        })
        .collect()
}

fn watcher_alive(id: &str) -> bool {
    !find_processes(&["__watch", id]).is_empty()
}

pub fn big_picture_running() -> bool {
    !find_processes(&["--big-picture"]).is_empty()
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub game_mode: bool,
    pub session_ready: bool,
    pub running_game: Option<Running>,
}

pub fn status() -> Status {
    Status {
        game_mode: big_picture_running() || in_gamescope(),
        session_ready: session_ready(),
        running_game: running_game(),
    }
}

/// Close every window, politely first. Returns the ones still open.
pub fn close_windows(timeout: Duration) -> Vec<hypr::Client> {
    let me = std::process::id();
    let targets: Vec<_> = hypr::clients().into_iter().filter(|c| c.pid != me).collect();
    for c in &targets {
        let _ = hypr::dispatch(&["closewindow", &format!("address:{}", c.address)]);
    }
    let start = Instant::now();
    loop {
        let left: Vec<_> = hypr::clients().into_iter().filter(|c| c.pid != me).collect();
        if left.is_empty() || start.elapsed() >= timeout {
            return left;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[derive(Debug, PartialEq)]
pub enum Entered {
    /// Hyprland is exiting; m3-session starts gamescope next.
    Session,
    /// gamescope started inside Hyprland.
    Nested,
}

/// gamescope arguments for Game Mode: the user's settings plus Steam
/// integration (`-e`, which enables our overlay and focus control) and the
/// MangoHud performance overlay when mangoapp is installed.
pub fn game_mode_args(config: &Config, display: Option<(u32, u32, u32)>, mangoapp: bool) -> Vec<String> {
    let mut args = config.gamescope.args(display);
    if !args.iter().any(|a| a == "-e" || a == "--steam") {
        args.insert(0, "-e".into());
    }
    if mangoapp && !args.iter().any(|a| a == "--mangoapp") {
        args.push("--mangoapp".into());
    }
    args
}

/// The MangoHud config file gamescope's mangoapp watches; starts hidden.
pub fn prepare_mangohud() -> std::path::PathBuf {
    let path = crate::gamescope::mangohud_config_path();
    let _ = crate::library::write_atomic(&path, crate::gamescope::mangohud_config(0));
    path
}

/// Enter Game Mode. In the m3-session login session this closes all apps
/// and leaves Hyprland so gamescope gets the GPU to itself; otherwise
/// gamescope runs fullscreen inside Hyprland.
pub fn enter(config: &Config) -> Result<Entered, String> {
    let display = if hypr::running() { hypr::focused_monitor() } else { None };
    let mangoapp = cmd::exists("mangoapp");
    let args = game_mode_args(config, display, mangoapp);
    if mangoapp {
        prepare_mangohud();
    }
    if session_ready() && hypr::running() {
        write_flag(&runtime_dir(), &args).map_err(|e| e.to_string())?;
        close_windows(Duration::from_secs(5));
        hypr::dispatch(&["exit"])?;
        return Ok(Entered::Session);
    }
    if !cmd::exists("gamescope") {
        return Err("gamescope isn't installed (pacman -S gamescope).".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if hypr::running() && config.performance_mode {
        let _ = hypr::performance_mode_on(hypr::current_syntax());
    }
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new("gamescope");
    cmd.args(&args)
        .arg("--")
        .arg(&exe)
        .arg("--big-picture")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    if mangoapp {
        cmd.env("MANGOHUD_CONFIGFILE", crate::gamescope::mangohud_config_path());
    }
    cmd.spawn().map_err(|e| format!("gamescope: {e}"))?;
    Ok(Entered::Nested)
}

/// Leave Game Mode from outside the big-picture UI: ask it to quit, which
/// ends gamescope (and in m3-session, brings Hyprland back).
pub fn exit() -> Result<(), String> {
    let pids = find_processes(&["--big-picture"]);
    if pids.is_empty() {
        return Err("Game Mode isn't running.".into());
    }
    for pid in pids {
        let _ = cmd::run("kill", &["-TERM", &pid.to_string()]);
    }
    Ok(())
}

/// Install the login session with one pkexec prompt: the session entry and
/// the `m3-session` script (needed when m3-games itself lives in ~/.local).
pub fn install_session() -> Result<(), String> {
    cmd::run("pkexec", &["sh", "-c", &install_script()]).map(|_| ())
}

fn install_script() -> String {
    r#"set -e
install -Dm755 /dev/stdin /usr/local/bin/m3-session.new <<'M3_SESSION_EOF'
__SCRIPT__M3_SESSION_EOF
mv /usr/local/bin/m3-session.new /usr/local/bin/m3-session
mkdir -p /usr/share/wayland-sessions
cat > /usr/share/wayland-sessions/m3-games-session.desktop <<'M3_DESKTOP_EOF'
__DESKTOP__M3_DESKTOP_EOF
"#
    .replace("__SCRIPT__", SESSION_SCRIPT)
    .replace("__DESKTOP__", SESSION_DESKTOP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!read_flag(dir.path()));
        write_flag(dir.path(), &["-f".into(), "-W".into(), "1920".into()]).unwrap();
        assert!(read_flag(dir.path()));
        assert_eq!(std::fs::read_to_string(dir.path().join("gamescope-args")).unwrap(), "-f\n-W\n1920\n");
    }

    #[test]
    fn game_mode_arguments() {
        let c = Config::default();
        let a = game_mode_args(&c, Some((1920, 1080, 60)), true);
        assert_eq!(a[0], "-e");
        assert!(a.contains(&"--mangoapp".to_string()));
        assert!(a.contains(&"-W".to_string()));
        let mut c2 = Config::default();
        c2.gamescope.extra = "--steam --mangoapp".into();
        let a = game_mode_args(&c2, None, true);
        assert!(!a.contains(&"-e".to_string()));
        assert_eq!(a.iter().filter(|x| *x == "--mangoapp").count(), 1);
        assert!(!game_mode_args(&c, None, false).contains(&"--mangoapp".to_string()));
    }

    #[test]
    fn running_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("running.json");
        std::fs::write(&p, r#"{"id":"steam:570","name":"Dota 2","pid":1}"#).unwrap();
        assert_eq!(read_running_at(&p).unwrap().id, "steam:570");
        std::fs::write(&p, "junk").unwrap();
        assert!(read_running_at(&p).is_none());
    }

    #[test]
    fn status_json_shape() {
        let s = Status { game_mode: false, session_ready: true, running_game: Some(Running { id: "a".into(), name: "A".into(), pid: 3, ..Default::default() }) };
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert_eq!(v, serde_json::json!({"game_mode": false, "session_ready": true, "running_game": {"id": "a", "name": "A", "pid": 3}}));
    }

    #[test]
    fn install_script_parses() {
        let script = install_script();
        assert!(script.contains("M3_SESSION_EOF\nmv "));
        let out = std::process::Command::new("sh").args(["-n", "-c", &script]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn session_files_are_embedded() {
        assert!(SESSION_SCRIPT.starts_with("#!/usr/bin/env bash"));
        assert!(SESSION_DESKTOP.contains("Exec=m3-session"));
    }
}
