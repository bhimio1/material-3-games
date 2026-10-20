//! The overlay listens on `$XDG_RUNTIME_DIR/m3-games/overlay.sock` for one
//! JSON message per line, from the big-picture UI and `m3-games overlay`.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Msg {
    Toggle,
    Show,
    Hide,
    Screenshot,
    CloseGame,
    /// The big-picture UI started a game (details are in running.json).
    GameStarted,
    GameEnded,
}

impl Msg {
    pub fn parse(word: &str) -> Option<Msg> {
        Some(match word {
            "toggle" => Msg::Toggle,
            "show" => Msg::Show,
            "hide" => Msg::Hide,
            "screenshot" => Msg::Screenshot,
            "close-game" => Msg::CloseGame,
            _ => return None,
        })
    }
}

pub fn socket_path() -> PathBuf {
    crate::session::runtime_dir().join("overlay.sock")
}

pub fn encode(msg: Msg) -> String {
    serde_json::to_string(&msg).unwrap_or_default() + "\n"
}

pub fn decode(line: &str) -> Option<Msg> {
    serde_json::from_str(line.trim()).ok()
}

/// Send one message to the overlay.
pub fn send_to(path: &Path, msg: Msg) -> Result<(), String> {
    let mut s = UnixStream::connect(path).map_err(|_| "The Game Mode overlay isn't running.".to_string())?;
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    s.write_all(encode(msg).as_bytes()).map_err(|e| e.to_string())
}

pub fn send(msg: Msg) -> Result<(), String> {
    send_to(&socket_path(), msg)
}

/// Listen on `path` (replacing a stale socket) and forward messages to the
/// returned channel; `wake` is called after each one.
pub fn serve(path: &Path, wake: impl Fn() + Send + Sync + 'static) -> Result<flume::Receiver<Msg>, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if UnixStream::connect(path).is_ok() {
        return Err("Another overlay is already running.".into());
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).map_err(|e| e.to_string())?;
    let (tx, rx) = flume::unbounded();
    let wake = std::sync::Arc::new(wake);
    std::thread::Builder::new()
        .name("overlay-ipc".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                let wake = wake.clone();
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    for line in BufReader::new(stream).lines().map_while(Result::ok) {
                        if let Some(m) = decode(&line) {
                            if tx.send(m).is_err() {
                                return;
                            }
                            wake();
                        }
                    }
                });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol() {
        assert_eq!(encode(Msg::Toggle), "{\"cmd\":\"toggle\"}\n");
        assert_eq!(decode("{\"cmd\":\"close_game\"}"), Some(Msg::CloseGame));
        assert_eq!(decode("garbage"), None);
        assert_eq!(Msg::parse("close-game"), Some(Msg::CloseGame));
        assert_eq!(Msg::parse("nope"), None);
    }

    #[test]
    fn socket_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("o.sock");
        assert!(send_to(&path, Msg::Show).is_err());
        let rx = serve(&path, || {}).unwrap();
        assert!(serve(&path, || {}).is_err(), "second overlay must refuse");
        send_to(&path, Msg::Toggle).unwrap();
        send_to(&path, Msg::GameStarted).unwrap();
        let mut got = vec![
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        ];
        got.sort_by_key(|m| format!("{m:?}"));
        assert_eq!(got, [Msg::GameStarted, Msg::Toggle]);
    }
}
