//! `m3-games __watch <id> <pid> <started>`: a detached helper started by a
//! desktop-mode launch. It waits for the game's window on Hyprland's event
//! socket, makes it fullscreen, turns on performance mode, and records play
//! time when the window closes.

use crate::config::Config;
use crate::hypr::{self, Event};
use crate::library::{self, Game, Launch};
use crate::session;
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const FIND_TIMEOUT: Duration = Duration::from_secs(180);
/// A matching window that closes faster than this was a splash/launcher.
const SPLASH: Duration = Duration::from_secs(8);

/// Process group of `pid`.
fn pgrp(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(2)?.parse().ok()
}

/// Is `pid` part of what we launched (descendant, or still in the process
/// group we created, even after the launcher itself exited)?
fn ours(pid: u32, launched: u32, tree: &HashSet<u32>) -> bool {
    pid != 0 && (tree.contains(&pid) || pgrp(pid) == Some(launched))
}

pub fn run(id: &str, launched: u32, started: i64) {
    let mut config = Config::load();
    let games = library::scan(&crate::sys::paths::home(), config.enabled());
    let Some(game) = games.into_iter().find(|g| g.id == id) else {
        return;
    };
    let opts = config.options(id);
    if !hypr::running() {
        wait_for_exit(&game, launched, started);
        return;
    }
    let Some(sock) = hypr::event_socket() else {
        wait_for_exit(&game, launched, started);
        return;
    };
    let Ok(stream) = UnixStream::connect(sock) else {
        wait_for_exit(&game, launched, started);
        return;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(stream);
    let hint = game.class_hint();
    let command_game = matches!(game.launch, Launch::Command { .. } | Launch::Manual(_));
    let syntax = hypr::current_syntax();
    let begin = Instant::now();
    let mut tracked: Option<(String, Instant)> = None;
    let mut line = String::new();

    loop {
        if tracked.is_none() && begin.elapsed() > FIND_TIMEOUT {
            break;
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // Hyprland went away.
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(_) => break,
        }
        match hypr::parse_event(line.trim_end()) {
            Event::OpenWindow { address, class, .. } if tracked.is_none() => {
                let tree = hypr::process_tree(launched);
                let pid = hypr::clients().into_iter().find(|c| c.address == address).map(|c| c.pid).unwrap_or(0);
                let matched = hypr::is_game_window(&class, pid, hint.as_deref(), |p| ours(p, launched, &tree))
                    // Last resort for games we run directly, when the launcher
                    // already exited: the first new window soon after launch.
                    || (hint.is_none()
                        && command_game
                        && begin.elapsed() < Duration::from_secs(60)
                        && !hypr::is_launcher_class(&class)
                        && tree.len() <= 1);
                if !matched {
                    continue;
                }
                if opts.force_fullscreen {
                    let _ = hypr::fullscreen_window(&address);
                }
                if config.performance_mode {
                    let _ = hypr::performance_mode_on(syntax);
                }
                if hint.is_none() && config.learn_class(&class) {
                    let _ = config.save();
                    let target = hypr::target(&crate::sys::paths::home());
                    if hypr::rules_installed(&target) {
                        let _ = hypr::write_rules(&target, &config.learned_classes, true);
                    }
                }
                tracked = Some((address, Instant::now()));
            }
            Event::CloseWindow { address } => {
                if let Some((addr, opened)) = &tracked {
                    if *addr == address {
                        let played = opened.elapsed();
                        if config.performance_mode {
                            let _ = hypr::performance_mode_off();
                        }
                        if played < SPLASH {
                            // Splash or launcher window: wait for the real one.
                            tracked = None;
                            continue;
                        }
                        library::record_play(id, started, (played.as_secs() + 30) / 60);
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    session::clear_running(id);
    let mut games = library::scan(&crate::sys::paths::home(), config.enabled());
    crate::covers::apply_cached(&mut games);
    library::write_cache(&games);
}

/// Without Hyprland: time the process group for games we run directly.
fn wait_for_exit(game: &Game, launched: u32, started: i64) {
    if matches!(game.launch, Launch::Command { .. } | Launch::Manual(_)) {
        let t = Instant::now();
        while hypr::alive(launched) {
            std::thread::sleep(Duration::from_secs(2));
        }
        library::record_play(&game.id, started, (t.elapsed().as_secs() + 30) / 60);
    }
    session::clear_running(&game.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_groups() {
        let me = std::process::id();
        assert!(pgrp(me).is_some());
        let tree: HashSet<u32> = [me].into();
        assert!(ours(me, 1, &tree));
        assert!(!ours(0, 1, &tree));
    }
}
