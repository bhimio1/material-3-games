//! Hyprland integration: static game window rules in a managed block of the
//! user's config, the event socket for catching a game's window, runtime
//! fullscreen/performance tweaks, and leaving the session for Game Mode.
//!
//! Hyprland 0.55+ reads Lua (`hyprland.lua`); older versions hyprlang
//! (`hyprland.conf`). On Lua configs `hyprctl keyword` is a no-op, so runtime
//! config goes through `hyprctl eval`. Dispatchers work on both.

use crate::sys::cmd;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const BEGIN: &str = ">>> m3-games >>>";
pub const END: &str = "<<< m3-games <<<";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    Lua,
    Conf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub syntax: Syntax,
    pub path: PathBuf,
    pub end4: bool,
}

/// Where the game rules go. end-4 dots load `custom/rules.*` after their own
/// rules, so ours survive dots updates there.
pub fn target(home: &Path) -> Target {
    let hypr = home.join(".config/hypr");
    let lua = hypr.join("hyprland.lua");
    let conf = hypr.join("hyprland.conf");
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    if lua.is_file() {
        let main = read(&lua);
        if main.contains("custom.rules") || main.contains("custom/rules") {
            return Target { syntax: Syntax::Lua, path: hypr.join("custom/rules.lua"), end4: true };
        }
        return Target { syntax: Syntax::Lua, path: lua, end4: false };
    }
    if conf.is_file() && read(&conf).contains("custom/rules.conf") {
        return Target { syntax: Syntax::Conf, path: hypr.join("custom/rules.conf"), end4: true };
    }
    Target { syntax: Syntax::Conf, path: conf, end4: false }
}

/// Inside a Hyprland session with hyprctl available.
pub fn running() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() && cmd::exists("hyprctl")
}

fn comment(syntax: Syntax) -> &'static str {
    match syntax {
        Syntax::Lua => "--",
        Syntax::Conf => "#",
    }
}

/// Window matches that get game rules: Steam games, gamescope, Windows
/// executables, and classes learned from games launched here.
pub fn game_classes(learned: &[String]) -> Vec<String> {
    let mut v = vec![r"^(steam_app_\d+)$".to_string(), "^(gamescope)$".to_string()];
    for c in learned {
        if c.starts_with("steam_app_") || c == "gamescope" {
            continue;
        }
        v.push(format!("^({})$", regex_escape(c)));
    }
    v
}

fn regex_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if r"\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The managed block, markers included.
pub fn render_rules(syntax: Syntax, learned: &[String]) -> String {
    let c = comment(syntax);
    let mut out = format!("{c} {BEGIN}\n{c} Game window rules from Material 3 Games. Edit them in its Settings.\n");
    let classes = game_classes(learned);
    match syntax {
        Syntax::Lua => {
            out.push_str("hl.config({ general = { allow_tearing = true } })\n");
            let effects = "immediate = true, idle_inhibit = \"fullscreen\", no_anim = true, no_blur = true, no_shadow = true";
            for class in &classes {
                out.push_str(&format!("hl.window_rule({{match = {{class = {}}}, {effects}}})\n", lua_string(class)));
            }
            out.push_str(&format!("hl.window_rule({{match = {{title = {}}}, {effects}}})\n", lua_string(r".*\.exe$")));
        }
        Syntax::Conf => {
            // Hyprland 0.53+ rule syntax.
            out.push_str("general:allow_tearing = true\n");
            let effects = "immediate on, idle_inhibit fullscreen, no_anim on, no_blur on, no_shadow on";
            for class in &classes {
                out.push_str(&format!("windowrule = match:class {class}, {effects}\n"));
            }
            out.push_str(&format!("windowrule = match:title {}, {effects}\n", r".*\.exe$"));
        }
    }
    out.push_str(&format!("{c} {END}\n"));
    out
}

/// Byte range of the block including its markers.
fn find_block(text: &str, syntax: Syntax) -> Option<(usize, usize)> {
    let c = comment(syntax);
    let start = text.find(&format!("{c} {BEGIN}"))?;
    let end_marker = format!("{c} {END}");
    let end = text[start..].find(&end_marker).map(|i| start + i + end_marker.len())?;
    let end = if text[end..].starts_with('\n') { end + 1 } else { end };
    Some((start, end))
}

pub fn has_block(text: &str, syntax: Syntax) -> bool {
    find_block(text, syntax).is_some()
}

/// Put `block` in place of the existing one (or append it); `None` removes it.
pub fn replace_block(text: &str, syntax: Syntax, block: Option<&str>) -> String {
    match (find_block(text, syntax), block) {
        (Some((s, e)), Some(b)) => format!("{}{}{}", &text[..s], b, &text[e..]),
        (Some((s, e)), None) => {
            let mut before = text[..s].to_string();
            // Drop the blank line we added in front of the block.
            if before.ends_with("\n\n") {
                before.pop();
            }
            format!("{before}{}", &text[e..])
        }
        (None, Some(b)) => {
            let mut out = text.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(b);
            out
        }
        (None, None) => text.to_string(),
    }
}

pub fn rules_installed(target: &Target) -> bool {
    std::fs::read_to_string(&target.path).is_ok_and(|t| has_block(&t, target.syntax))
}

/// Write (or with `install = false`, remove) the rules and reload Hyprland.
pub fn write_rules(target: &Target, learned: &[String], install: bool) -> Result<(), String> {
    let text = std::fs::read_to_string(&target.path).unwrap_or_default();
    let block = install.then(|| render_rules(target.syntax, learned));
    let mut new = replace_block(&text, target.syntax, block.as_deref());
    if text.is_empty() && target.end4 && target.syntax == Syntax::Lua && install {
        new = format!("-- This file will not be overwritten across dots-hyprland updates.\n\n{new}");
    }
    if new == text {
        return Ok(());
    }
    let real = std::fs::canonicalize(&target.path).unwrap_or_else(|_| target.path.clone());
    if let Some(dir) = real.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if real.is_file() {
        let _ = std::fs::copy(&real, real.with_extension("m3-games.bak"));
    }
    std::fs::write(&real, new).map_err(|e| e.to_string())?;
    if running() {
        reload()?;
    }
    Ok(())
}

pub fn reload() -> Result<(), String> {
    cmd::run("hyprctl", &["reload"]).map(|_| ())
}

pub fn dispatch(args: &[&str]) -> Result<(), String> {
    let mut a = vec!["dispatch"];
    a.extend_from_slice(args);
    let out = cmd::run("hyprctl", &a)?;
    let out = out.trim();
    if out.is_empty() || out == "ok" {
        Ok(())
    } else {
        Err(out.to_string())
    }
}

/// Lua or conf, judged by the files present.
pub fn current_syntax() -> Syntax {
    target(&crate::sys::paths::home()).syntax
}

/// Turn animations, blur and shadows off until the next reload.
pub fn performance_mode_on(syntax: Syntax) -> Result<(), String> {
    match syntax {
        Syntax::Lua => cmd::run(
            "hyprctl",
            &["eval", "hl.config({ animations = { enabled = false }, decoration = { blur = { enabled = false }, shadow = { enabled = false } } })"],
        )
        .map(|_| ()),
        Syntax::Conf => cmd::run(
            "hyprctl",
            &["--batch", "keyword animations:enabled 0; keyword decoration:blur:enabled 0; keyword decoration:shadow:enabled 0"],
        )
        .map(|_| ()),
    }
}

/// Undo [`performance_mode_on`] by reloading the config.
pub fn performance_mode_off() -> Result<(), String> {
    reload()
}

// ----------------------------------------------------------------- clients

#[derive(Clone, Debug, PartialEq)]
pub struct Client {
    pub address: String,
    pub class: String,
    pub title: String,
    pub pid: u32,
    /// 0 none, 1 maximized, 2 fullscreen (Hyprland's fullscreen mode).
    pub fullscreen: i64,
}

pub fn parse_clients(json: &str) -> Vec<Client> {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|c| {
            let fs = match c.get("fullscreen") {
                Some(Value::Bool(b)) => i64::from(*b) * 2,
                Some(v) => v.as_i64().unwrap_or(0),
                None => 0,
            };
            Some(Client {
                address: c.get("address")?.as_str()?.to_string(),
                class: c.get("class").and_then(Value::as_str).unwrap_or_default().to_string(),
                title: c.get("title").and_then(Value::as_str).unwrap_or_default().to_string(),
                pid: c.get("pid").and_then(Value::as_i64).unwrap_or(0).max(0) as u32,
                fullscreen: fs,
            })
        })
        .collect()
}

pub fn clients() -> Vec<Client> {
    cmd::run("hyprctl", &["clients", "-j"]).map(|s| parse_clients(&s)).unwrap_or_default()
}

/// Focused monitor's (width, height, refresh) for gamescope.
pub fn parse_focused_monitor(json: &str) -> Option<(u32, u32, u32)> {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(json) else {
        return None;
    };
    let m = list
        .iter()
        .find(|m| m.get("focused").and_then(Value::as_bool) == Some(true))
        .or_else(|| list.first())?;
    Some((
        m.get("width")?.as_u64()? as u32,
        m.get("height")?.as_u64()? as u32,
        m.get("refreshRate").and_then(Value::as_f64).unwrap_or(60.0).round() as u32,
    ))
}

pub fn focused_monitor() -> Option<(u32, u32, u32)> {
    cmd::run("hyprctl", &["monitors", "-j"]).ok().and_then(|s| parse_focused_monitor(&s))
}

/// Focus a window and make it fullscreen, without toggling it back out if
/// it already is.
pub fn fullscreen_window(address: &str) -> Result<(), String> {
    let addr = format!("address:{address}");
    dispatch(&["focuswindow", &addr])?;
    let already = clients().iter().any(|c| c.address == address && c.fullscreen >= 2);
    if !already {
        dispatch(&["fullscreen", "0"])?;
    }
    // Best effort; property names vary between versions.
    let _ = dispatch(&["setprop", &addr, "immediate", "1"]);
    let _ = dispatch(&["setprop", &addr, "noanim", "1"]);
    Ok(())
}

// ------------------------------------------------------------------ events

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    OpenWindow { address: String, workspace: String, class: String, title: String },
    CloseWindow { address: String },
    Other,
}

/// One line from `.socket2.sock`, e.g. `openwindow>>55d1c0,1,steam_app_570,Dota 2`.
/// Addresses come without `0x`; we add it to match `hyprctl clients`.
pub fn parse_event(line: &str) -> Event {
    let Some((name, data)) = line.split_once(">>") else {
        return Event::Other;
    };
    let addr = |a: &str| if a.starts_with("0x") { a.to_string() } else { format!("0x{a}") };
    match name {
        "openwindow" => {
            let mut parts = data.splitn(4, ',');
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(a), Some(w), Some(c), t) => Event::OpenWindow {
                    address: addr(a),
                    workspace: w.to_string(),
                    class: c.to_string(),
                    title: t.unwrap_or_default().to_string(),
                },
                _ => Event::Other,
            }
        }
        "closewindow" => Event::CloseWindow { address: addr(data) },
        _ => Event::Other,
    }
}

pub fn event_socket() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    [
        PathBuf::from(&runtime).join("hypr").join(&sig).join(".socket2.sock"),
        PathBuf::from("/tmp/hypr").join(&sig).join(".socket2.sock"),
    ]
    .into_iter()
    .find(|p| p.exists())
}

/// Classes of launcher/store windows that are never the game itself.
pub fn is_launcher_class(class: &str) -> bool {
    let c = class.to_lowercase();
    c == "steam"
        || c == "steamwebhelper"
        || c.starts_with("net.lutris")
        || c == "lutris"
        || c.contains("heroic")
        || c == "material_3_games"
        || c.is_empty()
}

/// Does a new window belong to the game we launched? With a class hint
/// (Steam games) only the class counts; otherwise the window's process must
/// be ours and not a launcher.
pub fn is_game_window(class: &str, pid: u32, hint: Option<&str>, ours: impl Fn(u32) -> bool) -> bool {
    if let Some(h) = hint {
        return class == h;
    }
    !is_launcher_class(class) && pid != 0 && ours(pid)
}

// --------------------------------------------------------------- processes

/// `pid` and all its descendants, from /proc.
pub fn process_tree(root: u32) -> HashSet<u32> {
    let mut children: std::collections::HashMap<u32, Vec<u32>> = Default::default();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for e in entries.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if let Some(ppid) = std::fs::read_to_string(e.path().join("stat")).ok().and_then(|s| parse_ppid(&s)) {
                children.entry(ppid).or_default().push(pid);
            }
        }
    }
    let mut out = HashSet::new();
    let mut stack = vec![root];
    while let Some(p) = stack.pop() {
        if out.insert(p) {
            if let Some(c) = children.get(&p) {
                stack.extend(c);
            }
        }
    }
    out
}

/// Parent pid from `/proc/<pid>/stat` (the comm field may contain spaces and
/// parentheses, so parse after the last `)`).
pub fn parse_ppid(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

pub fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_rules() {
        let b = render_rules(Syntax::Lua, &["hl2_linux".into(), "steam_app_9".into()]);
        assert!(b.starts_with("-- >>> m3-games >>>\n"));
        assert!(b.ends_with("-- <<< m3-games <<<\n"));
        assert!(b.contains("hl.config({ general = { allow_tearing = true } })"));
        assert!(b.contains(r#"hl.window_rule({match = {class = "^(steam_app_\\d+)$"}, immediate = true, idle_inhibit = "fullscreen", no_anim = true, no_blur = true, no_shadow = true})"#));
        assert!(b.contains(r#"class = "^(hl2_linux)$""#));
        assert!(!b.contains("steam_app_9"));
        assert!(b.contains(r#"title = ".*\\.exe$""#));
    }

    #[test]
    fn conf_rules() {
        let b = render_rules(Syntax::Conf, &["my.game".into()]);
        assert!(b.contains("general:allow_tearing = true\n"));
        assert!(b.contains("windowrule = match:class ^(steam_app_\\d+)$, immediate on, idle_inhibit fullscreen"));
        assert!(b.contains("match:class ^(my\\.game)$"));
    }

    #[test]
    fn block_replacement() {
        let user = "-- mine\nhl.window_rule({match = {class = \"kitty\"}, float = true})\n";
        let b1 = render_rules(Syntax::Lua, &[]);
        let t1 = replace_block(user, Syntax::Lua, Some(&b1));
        assert!(t1.starts_with(user));
        assert!(has_block(&t1, Syntax::Lua));
        let b2 = render_rules(Syntax::Lua, &["x".into()]);
        let t2 = replace_block(&t1, Syntax::Lua, Some(&b2));
        assert_eq!(t2.matches(BEGIN).count(), 1);
        assert!(t2.contains("^(x)$"));
        let t3 = replace_block(&t2, Syntax::Lua, None);
        assert_eq!(t3, user);
        assert_eq!(replace_block("", Syntax::Conf, Some("# b\n")), "# b\n");
    }

    #[test]
    fn targets() {
        let home = tempfile::tempdir().unwrap();
        let hypr = home.path().join(".config/hypr");
        std::fs::create_dir_all(&hypr).unwrap();
        assert_eq!(target(home.path()).syntax, Syntax::Conf);
        std::fs::write(hypr.join("hyprland.lua"), "if exists then require(\"custom.rules\") end").unwrap();
        let t = target(home.path());
        assert_eq!((t.syntax, t.end4), (Syntax::Lua, true));
        assert!(t.path.ends_with("custom/rules.lua"));

        write_rules(&t, &[], true).unwrap();
        let text = std::fs::read_to_string(&t.path).unwrap();
        assert!(text.starts_with("-- This file will not be overwritten"));
        assert!(rules_installed(&t));
        write_rules(&t, &[], false).unwrap();
        assert!(!rules_installed(&t));
    }

    #[test]
    fn events() {
        assert_eq!(
            parse_event("openwindow>>55d1c0e0,2,steam_app_570,Dota 2, the game"),
            Event::OpenWindow {
                address: "0x55d1c0e0".into(),
                workspace: "2".into(),
                class: "steam_app_570".into(),
                title: "Dota 2, the game".into()
            }
        );
        assert_eq!(parse_event("closewindow>>abc"), Event::CloseWindow { address: "0xabc".into() });
        assert_eq!(parse_event("workspace>>2"), Event::Other);
        assert_eq!(parse_event("garbage"), Event::Other);
        assert_eq!(parse_event("openwindow>>a,1"), Event::Other);
    }

    #[test]
    fn window_matching() {
        let tree: HashSet<u32> = [100, 101].into();
        let ours = |p: u32| tree.contains(&p);
        assert!(is_game_window("steam_app_570", 9, Some("steam_app_570"), ours));
        assert!(!is_game_window("steam", 9, Some("steam_app_570"), ours));
        assert!(is_game_window("celeste", 101, None, ours));
        assert!(!is_game_window("net.lutris.Lutris", 101, None, ours));
        assert!(!is_game_window("firefox", 555, None, ours));
    }

    #[test]
    fn clients_and_monitors() {
        let c = parse_clients(r#"[{"address":"0x1","class":"kitty","title":"t","pid":42,"fullscreen":2},{"address":"0x2","pid":-1,"fullscreen":false}]"#);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].fullscreen, 2);
        assert_eq!(c[1].pid, 0);
        assert!(parse_clients("nope").is_empty());
        let m = parse_focused_monitor(r#"[{"width":1920,"height":1080,"refreshRate":60.0,"focused":false},{"width":2560,"height":1440,"refreshRate":164.96,"focused":true}]"#);
        assert_eq!(m, Some((2560, 1440, 165)));
    }

    #[test]
    fn proc_stat() {
        assert_eq!(parse_ppid("123 (my (weird) game) S 77 123 123 0"), Some(77));
        let me = std::process::id();
        assert!(process_tree(me).contains(&me));
    }
}
