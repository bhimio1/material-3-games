//! Which Steam install and user we're working with, whether Steam is
//! running, restarting it around file edits, and backups.

use crate::library::vdf;
use crate::sys::cmd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// SteamID64 of account id 0.
pub const STEAMID64_BASE: u64 = 76_561_197_960_265_728;

/// The Steam install to edit: the first root that has a `userdata` folder.
pub fn root(home: &Path) -> Option<PathBuf> {
    let roots = crate::library::steam::roots(home);
    roots.iter().find(|r| r.join("userdata").is_dir()).cloned().or_else(|| roots.into_iter().next())
}

/// The most recently logged in user's SteamID64 from `loginusers.vdf`.
pub fn most_recent_steamid(root: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(root.join("config/loginusers.vdf")).ok()?;
    parse_loginusers(&text)
}

pub fn parse_loginusers(text: &str) -> Option<u64> {
    let doc = vdf::parse(text);
    let users = doc.get("users")?;
    let mut ids: Vec<(u64, bool, i64)> = users
        .entries()
        .iter()
        .filter_map(|(id, u)| {
            let id: u64 = id.parse().ok()?;
            let recent = u.get_str("MostRecent").is_some_and(|v| v == "1");
            let ts = u.get_str("Timestamp").and_then(|t| t.parse().ok()).unwrap_or(0);
            Some((id, recent, ts))
        })
        .collect();
    ids.sort_by_key(|(_, recent, ts)| (std::cmp::Reverse(*recent), std::cmp::Reverse(*ts)));
    ids.first().map(|(id, _, _)| *id)
}

pub fn account_id(steamid64: u64) -> u32 {
    steamid64.saturating_sub(STEAMID64_BASE) as u32
}

/// The user's `userdata/<accountid>` folder: the most recent login, else
/// the only (or first) one present.
pub fn user_dir(root: &Path) -> Option<PathBuf> {
    if let Some(id) = most_recent_steamid(root) {
        let d = root.join("userdata").join(account_id(id).to_string());
        if d.is_dir() {
            return Some(d);
        }
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("userdata"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().parse::<u32>().is_ok_and(|v| v > 0)))
        .collect();
    dirs.sort();
    dirs.into_iter().next()
}

pub fn user_account_id(root: &Path) -> Option<u32> {
    user_dir(root)?.file_name()?.to_string_lossy().parse().ok()
}

/// Steam rewrites shortcuts.vdf/config.vdf/localconfig.vdf when it exits,
/// so edits only stick while it isn't running.
pub fn steam_running() -> bool {
    cmd::run("pgrep", &["-x", "steam"]).is_ok_and(|o| !o.trim().is_empty())
        || pid_file_alive(&crate::sys::paths::home().join(".steam/steam.pid"))
}

fn pid_file_alive(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .is_some_and(|pid| {
            std::fs::read(format!("/proc/{pid}/comm")).is_ok_and(|c| String::from_utf8_lossy(&c).trim() == "steam")
        })
}

/// Shut Steam down and wait (up to `timeout`) for it to exit.
pub fn shutdown_steam(timeout: Duration) -> Result<(), String> {
    if !steam_running() {
        return Ok(());
    }
    let _ = cmd::spawn_detached("steam", &["-shutdown"]);
    let start = Instant::now();
    while start.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(500));
        if !steam_running() {
            // Give it a moment to finish writing its files.
            std::thread::sleep(Duration::from_secs(1));
            return Ok(());
        }
    }
    Err("Steam didn't close in time. Quit it from its menu and try again.".into())
}

/// Close Steam if needed, run `edit`, then start Steam again if it was
/// running before.
pub fn with_steam_closed<T>(edit: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let was_running = steam_running();
    if was_running {
        shutdown_steam(Duration::from_secs(30))?;
    }
    let out = edit();
    if was_running {
        let _ = cmd::spawn_detached("steam", &["-silent"]);
    }
    out
}

/// Copy `path` to `<name>.m3-games-<timestamp>.bak` beside it, keeping
/// the newest `keep` backups.
pub fn backup(path: &Path, keep: usize) -> std::io::Result<Option<PathBuf>> {
    if !path.is_file() {
        return Ok(None);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("."));
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let dest = dir.join(format!("{name}.m3-games-{stamp}.bak"));
    std::fs::copy(path, &dest)?;
    let prefix = format!("{name}.m3-games-");
    let mut old: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| {
            let n = n.to_string_lossy();
            n.starts_with(&prefix) && n.ends_with(".bak")
        }))
        .collect();
    old.sort();
    old.reverse();
    for stale in old.into_iter().skip(keep) {
        let _ = std::fs::remove_file(stale);
    }
    Ok(Some(dest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_users() {
        let text = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"old"
		"MostRecent"		"0"
		"Timestamp"		"1700000000"
	}
	"76561198000000002"
	{
		"AccountName"		"me"
		"MostRecent"		"1"
		"Timestamp"		"1600000000"
	}
}"#;
        let id = parse_loginusers(text).unwrap();
        assert_eq!(id, 76_561_198_000_000_002);
        assert_eq!(account_id(id), 39_734_274);
        assert!(parse_loginusers("junk").is_none());
    }

    #[test]
    fn user_dir_fallback() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("userdata/0")).unwrap();
        std::fs::create_dir_all(root.path().join("userdata/12345/config")).unwrap();
        assert!(user_dir(root.path()).unwrap().ends_with("userdata/12345"));
        assert_eq!(user_account_id(root.path()), Some(12345));
    }

    #[test]
    fn backups_rotate() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("shortcuts.vdf");
        assert!(backup(&f, 3).unwrap().is_none());
        std::fs::write(&f, b"x").unwrap();
        for _ in 0..5 {
            backup(&f, 3).unwrap();
            std::thread::sleep(Duration::from_millis(3));
        }
        let n = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(n, 4); // file + 3 backups
    }
}
