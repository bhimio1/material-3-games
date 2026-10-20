//! Tracking a game started from big picture inside gamescope: is it still
//! running (even after a store client handed it off), and stopping it.

use crate::session::Running;
use crate::sys::cmd;

/// Processes whose cgroup path ends in `unit`.
pub fn pids_in_unit(unit: &str) -> Vec<u32> {
    let me = std::process::id();
    std::fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
                .filter(|pid| *pid != me)
                .filter(|pid| std::fs::read_to_string(format!("/proc/{pid}/cgroup")).is_ok_and(|c| cgroup_has_unit(&c, unit)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn cgroup_has_unit(cgroup: &str, unit: &str) -> bool {
    cgroup.lines().any(|l| l.rsplit('/').next().is_some_and(|last| last == unit))
}

/// Processes of a Steam game (Steam puts SteamAppId in their environment).
pub fn steam_game_pids(appid: &str) -> Vec<u32> {
    let mut p = crate::procenv::with_env("SteamAppId", appid);
    if p.is_empty() {
        p = crate::procenv::with_env("SteamGameId", appid);
    }
    p
}

/// Is any part of the game still running? `launcher_alive` covers games
/// we run directly.
pub fn alive(r: &Running, steam_appid: Option<&str>, launcher_alive: bool) -> bool {
    if launcher_alive {
        return true;
    }
    if let Some(unit) = &r.unit {
        if !pids_in_unit(unit).is_empty() {
            return true;
        }
    }
    if let Some(a) = steam_appid {
        if !steam_game_pids(a).is_empty() {
            return true;
        }
    }
    false
}

/// Ask the game to quit (SIGTERM to its scope, Steam processes or group).
pub fn stop(r: &Running, steam_appid: Option<&str>) -> Result<(), String> {
    if let Some(unit) = &r.unit {
        if cmd::run("systemctl", &["--user", "kill", "--signal=SIGTERM", unit]).is_ok() {
            return Ok(());
        }
    }
    let mut pids: Vec<u32> = steam_appid.map(steam_game_pids).unwrap_or_default();
    if let Some(unit) = &r.unit {
        pids.extend(pids_in_unit(unit));
    }
    if pids.is_empty() && r.pid > 0 {
        // Our own launch: its process group.
        return cmd::run("kill", &["-TERM", "--", &format!("-{}", r.pid)]).map(|_| ());
    }
    if pids.is_empty() {
        return Err("The game isn't running.".into());
    }
    let args: Vec<String> = std::iter::once("-TERM".to_string()).chain(pids.iter().map(u32::to_string)).collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    cmd::run("kill", &refs).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgroups() {
        let cg = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-steam-app620-77.scope\n";
        assert!(cgroup_has_unit(cg, "app-steam-app620-77.scope"));
        assert!(!cgroup_has_unit(cg, "app-steam-app62-77.scope"));
        let r = Running { id: "x".into(), name: "X".into(), pid: 0, unit: Some("app-steam-app1-1.scope".into()), ..Default::default() };
        assert!(!alive(&r, None, false));
        assert!(alive(&r, None, true));
    }
}
