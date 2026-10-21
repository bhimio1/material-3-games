//! Material 3 Games — a game manager for Steam, Lutris, Heroic and your
//! own games, with a gamescope Game Mode, themed to match end4-pC.

mod app;
mod config;
mod covers;
mod gamepad;
mod gamescope;
mod gamesession;
mod hypr;
mod launch;
mod procenv;
mod library;
mod overlay;
mod session;
mod steam;
mod sys;
mod thumbs;
mod ui;
mod watch;

const USAGE: &str = "Usage: m3-games [COMMAND]

With no command, opens the library window.

Commands:
  --big-picture      Full-screen, controller-first library (used by Game Mode)
  list --json        Print the library as JSON
  launch <ID>        Start a game (fullscreen and tuned on Hyprland)
  game-mode          Enter Game Mode
  exit-game-mode     Leave Game Mode
  status --json      Print Game Mode state and the running game as JSON
  overlay <ACTION>   Game Mode overlay: toggle, show, hide, screenshot, close-game
  scheduler status|gaming|restore
                     sched-ext gaming scheduler via scxctl (Settings → CachyOS)
  steam-input on|off <ID>...
                     Add/remove Steam shortcuts so games get Steam Input
                     (closes and restarts Steam if it's running)
  -h, --help         Show this help
  -V, --version";

fn scan() -> Vec<library::Game> {
    let config = config::Config::load();
    let mut games = library::scan(&sys::paths::home(), config.enabled_local());
    covers::apply_cached(&mut games);
    library::write_cache(&games);
    games
}

fn run_gui(big_picture: bool) -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(if big_picture { "Game Mode" } else { "Games" })
        .with_app_id("material_3_games")
        .with_inner_size([1240.0, 820.0])
        .with_min_inner_size([760.0, 520.0]);
    // In gamescope the window is the whole screen anyway; elsewhere ask for
    // fullscreen (screenshots can opt out with M3_GAMES_WINDOWED).
    if big_picture && std::env::var_os("M3_GAMES_WINDOWED").is_none() {
        viewport = viewport.with_fullscreen(true);
    }
    // Development: a fixed window size for screenshots ("1240x1400").
    if let Some((w, h)) = std::env::var("M3_GAMES_WINDOW_SIZE").ok().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
    }) {
        viewport = viewport.with_inner_size([w, h]);
    }
    eframe::run_native(
        "material_3_games",
        eframe::NativeOptions { viewport, ..Default::default() },
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, big_picture)))),
    )
}

/// `m3-games steam-input on|off <id>...`
fn steam_input_cli(on: bool, ids: &[&str]) -> i32 {
    use steam::{account, shortcuts};
    let mut config = config::Config::load();
    let games = library::scan(&sys::paths::home(), config.enabled_local());
    let exe = std::env::current_exe().unwrap_or_else(|_| "m3-games".into());
    let mut specs = Vec::new();
    for id in ids {
        match games.iter().find(|g| g.id == *id) {
            Some(g) if g.steam_appid().is_some() => eprintln!("m3-games: {id} is a Steam game; Steam Input already applies"),
            Some(g) => specs.push(shortcuts::Spec::for_game(g, &exe)),
            None => eprintln!("m3-games: no game with id {id}"),
        }
    }
    let ids: Vec<String> = specs.iter().map(|s| s.game_id.clone()).collect();
    if ids.is_empty() {
        return 1;
    }
    let result = account::with_steam_closed(|| {
        let root = account::root(&sys::paths::home()).ok_or("Steam isn't installed")?;
        let user = account::user_dir(&root).ok_or("Log in to Steam once first")?;
        if on {
            shortcuts::apply(&user, &specs, &[])
        } else {
            shortcuts::apply(&user, &[], &ids)
        }
    });
    match result {
        Ok(map) => {
            for id in &ids {
                config.games.entry(id.clone()).or_default().steam_input = on;
                match map.get(id) {
                    Some(appid) => println!("{id}\tsteam://rungameid/{}", shortcuts::rungameid(*appid)),
                    None => println!("{id}\tremoved"),
                }
            }
            let _ = config.save();
            library::write_cache(&scan());
            0
        }
        Err(e) => {
            eprintln!("m3-games: {e}");
            1
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let code = match a.as_slice() {
        [] => run_gui(false).map(|_| 0).unwrap_or(1),
        ["--big-picture"] => run_gui(true).map(|_| 0).unwrap_or(1),
        ["list", "--json"] | ["list"] => {
            println!("{}", library::export_json(&scan()));
            0
        }
        ["launch", id] => {
            let config = config::Config::load();
            let games = scan();
            match games.iter().find(|g| g.id == *id) {
                Some(g) => match launch::launch_desktop(g, &config) {
                    Ok(_) => 0,
                    Err(e) => {
                        eprintln!("m3-games: {e}");
                        1
                    }
                },
                None => {
                    eprintln!("m3-games: no game with id {id}");
                    1
                }
            }
        }
        ["game-mode"] => match session::enter(&config::Config::load()) {
            Ok(session::Entered::Session) => 0,
            Ok(session::Entered::Nested) => 0,
            Err(e) => {
                eprintln!("m3-games: {e}");
                1
            }
        },
        ["exit-game-mode"] => match session::exit() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("m3-games: {e}");
                1
            }
        },
        ["status", "--json"] | ["status"] => {
            println!("{}", serde_json::to_string(&session::status()).unwrap_or_default());
            0
        }
        ["scheduler", what] => {
            let config = config::Config::load();
            let r = match *what {
                "status" => sys::scx::current().map(|s| println!("{}", s.label())),
                "gaming" => sys::scx::enter_gaming(&config),
                "restore" => sys::scx::restore(),
                _ => Err(USAGE.into()),
            };
            match r {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("m3-games: {e}");
                    1
                }
            }
        }
        ["__run", id] => {
            let config = config::Config::load();
            let games = library::scan(&sys::paths::home(), config.enabled_local());
            match games.iter().find(|g| g.id == *id) {
                Some(g) => launch::run_foreground(g, &config).unwrap_or_else(|e| {
                    eprintln!("m3-games: {e}");
                    1
                }),
                None => {
                    eprintln!("m3-games: no game with id {id}");
                    1
                }
            }
        }
        ["steam-input", state @ ("on" | "off"), ids @ ..] if !ids.is_empty() => steam_input_cli(*state == "on", ids),
        ["__overlay"] => overlay::run().map(|_| 0).unwrap_or(1),
        ["overlay", word] => match overlay::ipc::Msg::parse(word) {
            Some(m) => match overlay::ipc::send(m) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("m3-games: {e}");
                    1
                }
            },
            None => {
                eprintln!("m3-games: overlay takes toggle, show, hide, screenshot or close-game");
                2
            }
        },
        ["__watch", id, pid, started] => {
            watch::run(id, pid.parse().unwrap_or(0), started.parse().unwrap_or(0));
            0
        }
        ["-h" | "--help"] => {
            println!("{USAGE}");
            0
        }
        ["-V" | "--version"] => {
            println!("m3-games {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}
