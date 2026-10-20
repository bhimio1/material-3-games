//! sched-ext schedulers through scx_loader's `scxctl` (CachyOS ships both).
//!
//! The "Gaming scheduler" setting switches to a scheduler and mode (by
//! default scx_lavd in gaming mode) while Game Mode is on, or while a game
//! runs on the desktop, and puts the previous one back afterwards. What was
//! running before is kept in `$XDG_RUNTIME_DIR/m3-games/scx-previous` so
//! `m3-session` can restore it after gamescope exits, even if the
//! big-picture UI didn't get to.
//!
//! scxctl's output (scx-loader's `crates/scxctl/src/main.rs`):
//!   get   "no scx scheduler running"
//!         "running Lavd in Gaming mode"
//!         "running Lavd with its own defaults"
//!         "running Bpfland with arguments \"-k -m performance\""
//!   list  "supported schedulers: [\"bpfland\", \"lavd\", …]"
//! Modes are auto, gaming, powersave, lowlatency and server.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const SCXCTL: &str = "scxctl";

/// Modes scx_loader knows, as `--mode` takes them, with labels.
pub const MODES: &[(&str, &str)] = &[
    ("auto", "Auto"),
    ("gaming", "Gaming"),
    ("lowlatency", "Low latency"),
    ("powersave", "Power save"),
    ("server", "Server"),
];

pub fn available() -> bool {
    super::cmd::exists(SCXCTL)
}

/// What scx_loader is running.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// The kernel's own scheduler.
    None,
    /// A scheduler (without the `scx_` prefix) in a mode.
    Mode { sched: String, mode: String },
    /// A scheduler with custom arguments, as scxctl prints them (shell words).
    Args { sched: String, args: String },
}

impl State {
    pub fn sched(&self) -> Option<&str> {
        match self {
            State::None => None,
            State::Mode { sched, .. } | State::Args { sched, .. } => Some(sched),
        }
    }

    pub fn label(&self) -> String {
        match self {
            State::None => "Default (no sched-ext scheduler)".into(),
            State::Mode { sched, mode } => format!("scx_{sched} · {}", mode_label(mode)),
            State::Args { sched, args } => format!("scx_{sched} · {args}"),
        }
    }

    /// One line for the previous-state file.
    pub fn to_line(&self) -> String {
        match self {
            State::None => "none".into(),
            State::Mode { sched, mode } => format!("mode {sched} {mode}"),
            State::Args { sched, args } => format!("args {sched} {args}"),
        }
    }

    pub fn from_line(line: &str) -> Option<State> {
        let line = line.trim();
        if line == "none" {
            return Some(State::None);
        }
        let (kind, rest) = line.split_once(' ')?;
        let (sched, value) = rest.split_once(' ')?;
        let sched = sched.to_string();
        match kind {
            "mode" => Some(State::Mode { sched, mode: value.trim().to_string() }),
            "args" => Some(State::Args { sched, args: value.trim().to_string() }),
            _ => None,
        }
    }
}

pub fn mode_label(mode: &str) -> &str {
    MODES.iter().find(|(m, _)| *m == mode).map(|(_, l)| *l).unwrap_or(mode)
}

/// `CamelCase` or `scx_name` → the lowercase name scxctl takes.
fn norm_sched(s: &str) -> String {
    let s = s.trim().trim_matches('"').to_ascii_lowercase();
    s.strip_prefix("scx_").map(str::to_string).unwrap_or(s)
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Parse `scxctl get`. `None` when the output isn't recognised.
pub fn parse_get(text: &str) -> Option<State> {
    let text = strip_ansi(text);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("no ") && lower.contains("running") {
        return Some(State::None);
    }
    let rest = line.strip_prefix("running ")?;
    if let Some((sched, args)) = rest.split_once(" with arguments ") {
        let args = args.trim();
        let args = args.strip_prefix('"').and_then(|a| a.strip_suffix('"')).unwrap_or(args);
        return Some(State::Args { sched: norm_sched(sched), args: args.to_string() });
    }
    if let Some((sched, mode)) = rest.split_once(" in ") {
        let mode = mode.trim().trim_end_matches(" mode").trim();
        return Some(State::Mode { sched: norm_sched(sched), mode: mode.to_ascii_lowercase() });
    }
    // "running Lavd with its own defaults", or a bare name.
    let sched = rest.split_whitespace().next()?;
    Some(State::Mode { sched: norm_sched(sched), mode: "auto".into() })
}

/// Parse `scxctl list`.
pub fn parse_list(text: &str) -> Vec<String> {
    let text = strip_ansi(text);
    let inner = match (text.find('['), text.rfind(']')) {
        (Some(a), Some(b)) if a < b => text[a + 1..b].to_string(),
        _ => text.split_once(':').map(|(_, r)| r.to_string()).unwrap_or(text.clone()),
    };
    let mut out: Vec<String> = inner
        .split([',', ' ', '\n', '\t'])
        .map(norm_sched)
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .collect();
    out.dedup();
    out
}

fn scxctl(args: &[&str]) -> Result<String, String> {
    let out = Command::new(SCXCTL)
        .args(args)
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{SCXCTL}: {e}"))?;
    let stdout = strip_ansi(&String::from_utf8_lossy(&out.stdout));
    if out.status.success() {
        Ok(stdout)
    } else {
        let err = strip_ansi(&String::from_utf8_lossy(&out.stderr));
        let msg = err.lines().chain(stdout.lines()).map(str::trim).find(|l| !l.is_empty()).unwrap_or("failed").to_string();
        Err(format!("{SCXCTL}: {msg}"))
    }
}

pub fn current() -> Result<State, String> {
    let out = scxctl(&["get"])?;
    parse_get(&out).ok_or_else(|| format!("{SCXCTL}: unexpected output: {}", out.trim()))
}

pub fn supported() -> Vec<String> {
    scxctl(&["list"]).map(|o| parse_list(&o)).unwrap_or_default()
}

/// What the Game Mode overlay shows.
#[derive(Clone, Debug)]
pub struct Info {
    pub current: State,
    pub supported: Vec<String>,
}

/// `None` when scxctl isn't installed or scx_loader isn't reachable.
pub fn info() -> Option<Info> {
    if !available() {
        return None;
    }
    let current = current().ok()?;
    Some(Info { current, supported: supported() })
}

/// The scxctl arguments that bring about `target` from `from`.
pub fn apply_args(from: &State, target: &State) -> Vec<String> {
    let verb = if *from == State::None { "start" } else { "switch" };
    let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect();
    match target {
        State::None => v(&["stop"]),
        State::Mode { sched, mode } => v(&[verb, "--sched", sched, "--mode", mode]),
        State::Args { sched, args } => v(&[verb, "--sched", sched, "--args", args]),
    }
}

/// Switch to `target` (a no-op when it's already running).
pub fn apply(target: &State) -> Result<(), String> {
    let from = current()?;
    if from == *target {
        return Ok(());
    }
    let args = apply_args(&from, target);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    scxctl(&refs).map(|_| ())
}

fn previous_path() -> PathBuf {
    crate::session::runtime_dir().join("scx-previous")
}

/// The configured gaming scheduler, if the setting is on and scxctl is here.
pub fn gaming_target(config: &crate::config::Config) -> Option<State> {
    if !config.scx_gaming || config.scx_sched.trim().is_empty() || !available() {
        return None;
    }
    Some(State::Mode { sched: norm_sched(&config.scx_sched), mode: config.scx_mode.trim().to_string() })
}

/// Switch to the gaming scheduler, remembering what ran before. Only the
/// first call saves the previous state, so nested uses (a desktop game
/// while in Game Mode) restore to what was there originally.
pub fn enter_gaming(config: &crate::config::Config) -> Result<(), String> {
    let Some(target) = gaming_target(config) else {
        return Ok(());
    };
    enter_at(&previous_path(), &target)
}

fn enter_at(file: &Path, target: &State) -> Result<(), String> {
    let now = current()?;
    if !file.exists() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(file, now.to_line() + "\n").map_err(|e| e.to_string())?;
    }
    if now == *target {
        return Ok(());
    }
    let args = apply_args(&now, target);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    scxctl(&refs).map(|_| ())
}

/// Put back the scheduler saved by [`enter_gaming`], if any. Falls back to
/// scx_loader's configured default when the exact state can't be restored.
pub fn restore() -> Result<(), String> {
    let file = previous_path();
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Ok(());
    };
    let _ = std::fs::remove_file(&file);
    if !available() {
        return Ok(());
    }
    match State::from_line(&text) {
        Some(prev) => apply(&prev).or_else(|e| scxctl(&["restore"]).map(|_| ()).map_err(|_| e)),
        None => scxctl(&["restore"]).map(|_| ()),
    }
}

/// Run `f` off the UI thread; scxctl waits on D-Bus.
pub fn in_background(f: impl FnOnce() -> Result<(), String> + Send + 'static) {
    std::thread::spawn(move || {
        if let Err(e) = f() {
            eprintln!("m3-games: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_output() {
        assert_eq!(parse_get("no scx scheduler running\n"), Some(State::None));
        assert_eq!(
            parse_get("running Lavd in Gaming mode\n"),
            Some(State::Mode { sched: "lavd".into(), mode: "gaming".into() })
        );
        assert_eq!(
            parse_get("running P2DQ in LowLatency mode"),
            Some(State::Mode { sched: "p2dq".into(), mode: "lowlatency".into() })
        );
        assert_eq!(
            parse_get("running Bpfland with its own defaults"),
            Some(State::Mode { sched: "bpfland".into(), mode: "auto".into() })
        );
        assert_eq!(
            parse_get("running Bpfland with arguments \"-k -m performance\""),
            Some(State::Args { sched: "bpfland".into(), args: "-k -m performance".into() })
        );
        // Older scxctl builds printed the scx_ name.
        assert_eq!(
            parse_get("running scx_rusty in PowerSave mode"),
            Some(State::Mode { sched: "rusty".into(), mode: "powersave".into() })
        );
        assert_eq!(
            parse_get("\u{1b}[1mrunning\u{1b}[0m Lavd in Auto mode"),
            Some(State::Mode { sched: "lavd".into(), mode: "auto".into() })
        );
        assert_eq!(parse_get(""), None);
        assert_eq!(parse_get("error: couldn't connect"), None);
    }

    #[test]
    fn list_output() {
        assert_eq!(
            parse_list("supported schedulers: [\"bpfland\", \"cosmos\", \"flash\", \"lavd\", \"p2dq\", \"tickless\", \"rustland\", \"rusty\"]\n"),
            ["bpfland", "cosmos", "flash", "lavd", "p2dq", "tickless", "rustland", "rusty"]
        );
        assert_eq!(parse_list("supported schedulers: []"), Vec::<String>::new());
        assert_eq!(parse_list("scx_lavd scx_bpfland"), ["lavd", "bpfland"]);
        assert_eq!(parse_list(""), Vec::<String>::new());
    }

    #[test]
    fn state_lines() {
        for s in [
            State::None,
            State::Mode { sched: "lavd".into(), mode: "gaming".into() },
            State::Args { sched: "bpfland".into(), args: "-k -m 'a b'".into() },
        ] {
            assert_eq!(State::from_line(&s.to_line()), Some(s));
        }
        assert_eq!(State::from_line("junk"), None);
        assert_eq!(State::from_line(""), None);
    }

    #[test]
    fn commands() {
        let lavd = State::Mode { sched: "lavd".into(), mode: "gaming".into() };
        assert_eq!(apply_args(&State::None, &lavd), ["start", "--sched", "lavd", "--mode", "gaming"]);
        let other = State::Mode { sched: "bpfland".into(), mode: "auto".into() };
        assert_eq!(apply_args(&other, &lavd), ["switch", "--sched", "lavd", "--mode", "gaming"]);
        assert_eq!(apply_args(&lavd, &State::None), ["stop"]);
        let args = State::Args { sched: "bpfland".into(), args: "-k -m performance".into() };
        assert_eq!(apply_args(&lavd, &args), ["switch", "--sched", "bpfland", "--args", "-k -m performance"]);
    }

    #[test]
    fn labels() {
        assert_eq!(State::Mode { sched: "lavd".into(), mode: "lowlatency".into() }.label(), "scx_lavd · Low latency");
        assert_eq!(norm_sched("scx_LAVD"), "lavd");
    }
}
