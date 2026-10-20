//! Starting games: the command line for each source with the per-game
//! wrappers (gamemoderun, MangoHud, gamescope), and the desktop-mode flow
//! that hands the window over to a background watcher.

use crate::config::{Config, GameOptions};
use crate::library::manual::Runner;
use crate::library::{desktop::split_exec, Game, Launch};
use crate::session;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Where a game is being started from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// From Hyprland: rules and the window watcher apply.
    Desktop,
    /// Inside Game Mode's gamescope: no compositor rules needed.
    BigPicture,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub dir: Option<PathBuf>,
    /// Launch goes through a store client that returns immediately, so the
    /// launched process isn't the game.
    pub via_client: bool,
}

fn env_pairs(lines: &[String]) -> Vec<(String, String)> {
    lines
        .iter()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, v)| (k.trim().to_string(), v.to_string()))
        .collect()
}

/// Launching through the game's Steam shortcut, so Steam Input applies.
pub fn uses_steam_shortcut(game: &Game, opts: &GameOptions) -> bool {
    opts.steam_input && game.shortcut_appid.is_some() && game.steam_appid().is_none()
}

/// Build the command for `game`. `display` is the monitor size for
/// per-game gamescope.
pub fn plan(game: &Game, opts: &GameOptions, config: &Config, ctx: Context, display: Option<(u32, u32, u32)>) -> Plan {
    let mut p = Plan::default();
    if let Some(appid) = game.shortcut_appid.filter(|_| uses_steam_shortcut(game, opts)) {
        // Steam runs `m3-games __run <id>`, which applies the options below.
        let url = format!("steam://rungameid/{}", crate::steam::shortcuts::rungameid(appid));
        p.argv = if ctx == Context::BigPicture { vec!["steam".into(), "-silent".into(), url] } else { vec!["steam".into(), url] };
        p.via_client = true;
        return p;
    }
    match &game.launch {
        Launch::Steam { appid } => {
            p.argv = if ctx == Context::BigPicture {
                vec!["steam".into(), "-silent".into(), format!("steam://rungameid/{appid}")]
            } else {
                vec!["steam".into(), format!("steam://rungameid/{appid}")]
            };
            p.via_client = true;
        }
        Launch::Lutris { id } => {
            p.argv = vec!["lutris".into(), format!("lutris:rungameid/{id}")];
            p.via_client = true;
        }
        Launch::Heroic { runner, app_name } => {
            p.argv = vec!["xdg-open".into(), format!("heroic://launch/{runner}/{app_name}")];
            p.via_client = true;
        }
        Launch::Command { argv, dir } => {
            p.argv = argv.clone();
            p.dir = dir.clone();
        }
        Launch::Manual(m) => {
            let args = split_exec(&m.args);
            match m.runner {
                Runner::Native => {
                    p.argv = vec![m.exe.clone()];
                }
                Runner::Umu => {
                    p.argv = vec!["umu-run".into(), m.exe.clone()];
                    p.env.push(("WINEPREFIX".into(), m.prefix().to_string_lossy().into_owned()));
                    p.env.push(("GAMEID".into(), "umu-default".into()));
                    p.env.push(("PROTONPATH".into(), if m.proton.is_empty() { crate::steam::appconfig::default_proton() } else { m.proton.clone() }));
                }
                Runner::Wine => {
                    p.argv = vec!["wine".into(), m.exe.clone()];
                    p.env.push(("WINEPREFIX".into(), m.prefix().to_string_lossy().into_owned()));
                }
            }
            p.argv.extend(args);
            p.env.extend(env_pairs(&m.env));
            p.dir = Some(PathBuf::from(&m.dir))
                .filter(|d| !m.dir.is_empty() && d.is_dir())
                .or_else(|| PathBuf::from(&m.exe).parent().map(PathBuf::from).filter(|d| d.is_dir()));
        }
    }
    p.env.extend(env_pairs(&opts.env));
    // Wrappers only work when we run the game ourselves.
    if !p.via_client {
        if opts.mangohud {
            p.argv.insert(0, "mangohud".into());
        }
        if opts.gamescope && ctx == Context::Desktop {
            let mut gs = vec!["gamescope".to_string()];
            gs.extend(config.gamescope.args(display));
            gs.push("--".into());
            gs.append(&mut p.argv);
            p.argv = gs;
        }
        if opts.gamemode {
            p.argv.insert(0, "gamemoderun".into());
        }
        if opts.cachyos_perf {
            p.argv.insert(0, crate::steam::appconfig::GAME_PERFORMANCE.into());
        }
    }
    p
}

/// Spawn the plan in its own process group.
pub fn spawn(p: &Plan) -> Result<std::process::Child, String> {
    use std::os::unix::process::CommandExt;
    let (prog, args) = p.argv.split_first().ok_or("nothing to run")?;
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .envs(p.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(d) = &p.dir {
        cmd.current_dir(d);
    }
    cmd.spawn().map_err(|e| format!("{prog}: {e}"))
}

/// `m3-games __run <id>`: what a Steam shortcut runs. Starts the game with
/// its options and stays in the foreground until it's over, so Steam keeps
/// Steam Input and its overlay active for the whole session.
pub fn run_foreground(game: &Game, config: &Config) -> Result<i32, String> {
    let mut opts = config.options(&game.id);
    // Steam is already the launcher; don't loop back through it.
    opts.steam_input = false;
    let ctx = if crate::hypr::running() { Context::Desktop } else { Context::BigPicture };
    let display = if ctx == Context::Desktop { crate::hypr::focused_monitor() } else { None };
    let p = plan(game, &opts, config, ctx, display);
    let mut child = spawn_foreground(&p)?;
    let started = now();
    let t = std::time::Instant::now();
    let status = child.wait().map_err(|e| e.to_string())?;
    if p.via_client {
        // Lutris/Heroic hand the game off and may exit early. Steam puts
        // SteamGameId in our environment and the game inherits it, so wait
        // until nothing with our SteamGameId is left.
        if let Ok(gid) = std::env::var("SteamGameId") {
            let mut idle = 0;
            while idle < 3 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if crate::procenv::with_env("SteamGameId", &gid).is_empty() {
                    idle += 1;
                } else {
                    idle = 0;
                }
            }
        }
    }
    crate::library::record_play(&game.id, started, (t.elapsed().as_secs() + 30) / 60);
    Ok(status.code().unwrap_or(0))
}

/// Like [`spawn`], but keeping our stdio and process group (Steam tracks
/// the shortcut by its process tree).
fn spawn_foreground(p: &Plan) -> Result<std::process::Child, String> {
    let (prog, args) = p.argv.split_first().ok_or("nothing to run")?;
    let mut cmd = Command::new(prog);
    cmd.args(args).envs(p.env.iter().map(|(k, v)| (k, v)));
    if let Some(d) = &p.dir {
        cmd.current_dir(d);
    }
    cmd.spawn().map_err(|e| format!("{prog}: {e}"))
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Desktop-mode launch: start the game, record it, and leave a detached
/// watcher to fullscreen its window and track play time.
pub fn launch_desktop(game: &Game, config: &Config) -> Result<u32, String> {
    let opts = config.options(&game.id);
    let display = if crate::hypr::running() { crate::hypr::focused_monitor() } else { None };
    let p = plan(game, &opts, config, Context::Desktop, display);
    let mut child = spawn(&p)?;
    let pid = child.id();
    let started = now();
    crate::library::record_play(&game.id, started, 0);
    session::write_running(&game.id, &game.name, pid);
    // Reap the launcher in the background so it doesn't linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let _ = Command::new(exe)
        .args(["__watch", &game.id, &pid.to_string(), &started.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::manual::ManualGame;
    use crate::library::{test_game, Source};

    #[test]
    fn store_plans() {
        let cfg = Config::default();
        let mut g = test_game("steam:570", "Dota", None);
        g.source = Source::Steam;
        g.launch = Launch::Steam { appid: "570".into() };
        let opts = GameOptions { gamemode: true, ..Default::default() };
        let p = plan(&g, &opts, &cfg, Context::Desktop, None);
        assert_eq!(p.argv, ["steam", "steam://rungameid/570"]);
        assert!(p.via_client);
        let p = plan(&g, &opts, &cfg, Context::BigPicture, None);
        assert_eq!(p.argv, ["steam", "-silent", "steam://rungameid/570"]);
        g.launch = Launch::Lutris { id: 3 };
        assert_eq!(plan(&g, &opts, &cfg, Context::Desktop, None).argv, ["lutris", "lutris:rungameid/3"]);
    }

    #[test]
    fn wrappers() {
        let cfg = Config::default();
        let g = test_game("desktop:x", "X", None);
        let opts = GameOptions { gamemode: true, mangohud: true, gamescope: true, env: vec!["A=1".into()], ..Default::default() };
        let p = plan(&g, &opts, &cfg, Context::Desktop, Some((1920, 1080, 144)));
        assert_eq!(
            p.argv,
            ["gamemoderun", "gamescope", "-f", "-W", "1920", "-H", "1080", "-r", "144", "--adaptive-sync", "--", "mangohud", "true"]
        );
        assert_eq!(p.env, [("A".to_string(), "1".to_string())]);
        // No nested gamescope inside Game Mode.
        let p = plan(&g, &opts, &cfg, Context::BigPicture, None);
        assert_eq!(p.argv, ["gamemoderun", "mangohud", "true"]);
        // CachyOS's wrapper goes outermost and can be combined with GameMode.
        let opts = GameOptions { gamemode: true, cachyos_perf: true, ..Default::default() };
        let p = plan(&g, &opts, &cfg, Context::BigPicture, None);
        assert_eq!(p.argv, ["game-performance", "gamemoderun", "true"]);
    }

    #[test]
    fn manual_windows_game() {
        let cfg = Config::default();
        let mut m = ManualGame::new("Tool", "/nonexistent/tool.exe");
        m.args = "-windowed \"two words\"".into();
        let g = m.into_game();
        let p = plan(&g, &GameOptions::default(), &cfg, Context::Desktop, None);
        assert_eq!(p.argv, ["umu-run", "/nonexistent/tool.exe", "-windowed", "two words"]);
        assert!(p.env.iter().any(|(k, v)| k == "WINEPREFIX" && v.ends_with("Games/m3-games/prefixes/tool")));
        assert!(p.env.iter().any(|(k, v)| k == "PROTONPATH" && *v == crate::steam::appconfig::default_proton()));
    }

    #[test]
    fn steam_input_route() {
        let cfg = Config::default();
        let mut g = test_game("lutris:3", "Celeste", None);
        g.launch = Launch::Lutris { id: 3 };
        g.shortcut_appid = Some(0x8000_0001);
        let mut opts = GameOptions::default();
        assert_eq!(plan(&g, &opts, &cfg, Context::Desktop, None).argv, ["lutris", "lutris:rungameid/3"]);
        opts.steam_input = true;
        let p = plan(&g, &opts, &cfg, Context::Desktop, None);
        assert_eq!(p.argv, ["steam", "steam://rungameid/9223372041183297536"]);
        assert!(p.via_client);
        // Steam games never go through a shortcut.
        g.launch = Launch::Steam { appid: "570".into() };
        assert!(!uses_steam_shortcut(&g, &opts));
    }

    #[test]
    fn spawning() {
        let p = Plan { argv: vec!["true".into()], ..Default::default() };
        let mut c = spawn(&p).unwrap();
        assert!(c.wait().unwrap().success());
        assert!(spawn(&Plan::default()).is_err());
    }
}
