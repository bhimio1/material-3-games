//! Material 3 Games — a game manager for Steam, Lutris, Heroic and your
//! own games, with a gamescope Game Mode, themed to match end4-pC.

mod app;
mod config;
mod covers;
mod gamepad;
mod hypr;
mod launch;
mod library;
mod session;
mod sys;
mod theme;
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
  -h, --help         Show this help
  -V, --version";

fn scan() -> Vec<library::Game> {
    let config = config::Config::load();
    let mut games = library::scan(&sys::paths::home(), config.enabled());
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
    eframe::run_native(
        "material_3_games",
        eframe::NativeOptions { viewport, ..Default::default() },
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, big_picture)))),
    )
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
