//! Lutris: games from its SQLite database (`pga.db`), cover art and banners
//! from its data folders. Native and Flatpak installs are both read.

use super::{Game, Launch, Source};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Lutris data folders that exist.
pub fn roots(home: &Path) -> Vec<PathBuf> {
    [
        home.join(".local/share/lutris"),
        home.join(".var/app/net.lutris.Lutris/data/lutris"),
    ]
    .into_iter()
    .filter(|p| p.join("pga.db").is_file())
    .collect()
}

#[derive(Debug, Default, PartialEq)]
pub struct Row {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub runner: Option<String>,
    pub installed: bool,
    pub last_played: Option<i64>,
    pub playtime_hours: Option<f64>,
    pub directory: Option<String>,
}

fn text(v: ValueRef<'_>) -> Option<String> {
    match v {
        ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Integer(i) => Some(i.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        _ => None,
    }
}

fn int(v: ValueRef<'_>) -> Option<i64> {
    match v {
        ValueRef::Integer(i) => Some(i),
        ValueRef::Real(f) => Some(f as i64),
        ValueRef::Text(t) => std::str::from_utf8(t).ok()?.trim().parse().ok(),
        _ => None,
    }
}

fn real(v: ValueRef<'_>) -> Option<f64> {
    match v {
        ValueRef::Integer(i) => Some(i as f64),
        ValueRef::Real(f) => Some(f),
        ValueRef::Text(t) => std::str::from_utf8(t).ok()?.trim().parse().ok(),
        _ => None,
    }
}

/// Read every game row. Columns are looked up by name so schema changes
/// across Lutris versions don't break it.
pub fn read_db(path: &Path) -> rusqlite::Result<Vec<Row>> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(500))?;
    let mut stmt = conn.prepare("SELECT * FROM games")?;
    let cols: HashMap<String, usize> = stmt
        .column_names()
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_string(), i))
        .collect();
    let col = |name: &str| cols.get(name).copied();
    let (c_id, c_name, c_slug, c_runner, c_inst, c_last, c_play, c_dir) = (
        col("id"),
        col("name"),
        col("slug"),
        col("runner"),
        col("installed"),
        col("lastplayed"),
        col("playtime"),
        col("directory"),
    );
    let rows = stmt.query_map([], |r| {
        let get = |c: Option<usize>| c.and_then(|i| r.get_ref(i).ok());
        Ok(Row {
            id: get(c_id).and_then(int).unwrap_or(0),
            name: get(c_name).and_then(text).unwrap_or_default(),
            slug: get(c_slug).and_then(text).unwrap_or_default(),
            runner: get(c_runner).and_then(text).filter(|s| !s.is_empty()),
            installed: get(c_inst).and_then(int).unwrap_or(0) != 0,
            last_played: get(c_last).and_then(int).filter(|t| *t > 0),
            playtime_hours: get(c_play).and_then(real),
            directory: get(c_dir).and_then(text).filter(|s| !s.is_empty()),
        })
    })?;
    Ok(rows.flatten().filter(|r| r.id > 0 && !r.name.is_empty()).collect())
}

fn image(root: &Path, home: &Path, kind: &str, slug: &str) -> Option<PathBuf> {
    let mut dirs = vec![root.join(kind)];
    if kind == "coverart" {
        dirs.push(home.join(".cache/lutris/coverart"));
    }
    for dir in dirs {
        for ext in ["jpg", "png", "webp"] {
            let p = dir.join(format!("{slug}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn scan(home: &Path) -> Vec<Game> {
    let mut games = Vec::new();
    for root in roots(home) {
        let rows = match read_db(&root.join("pga.db")) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("lutris: {}: {e}", root.display());
                continue;
            }
        };
        for r in rows {
            // Steam games imported into Lutris are already listed from Steam.
            if r.runner.as_deref() == Some("steam") {
                continue;
            }
            games.push(Game {
                id: format!("lutris:{}", r.id),
                cover: image(&root, home, "coverart", &r.slug),
                hero: image(&root, home, "banners", &r.slug),
                cover_url: None,
                last_played: r.last_played,
                playtime_minutes: r.playtime_hours.map(|h| (h * 60.0).round() as u64).filter(|m| *m > 0),
                installed: r.installed,
                install_dir: r.directory.as_ref().map(PathBuf::from),
                launch: Launch::Lutris { id: r.id },
                name: r.name,
                source: Source::Lutris,
            });
        }
    }
    games
}

#[cfg(test)]
pub(crate) fn make_test_db(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE games (id INTEGER PRIMARY KEY, name TEXT, slug TEXT, runner TEXT, platform TEXT,
            directory TEXT, installed INTEGER, lastplayed INTEGER, playtime REAL, configpath TEXT);
         INSERT INTO games VALUES (1, 'Celeste', 'celeste', 'linux', 'Linux', '/games/celeste', 1, 1712000000, 2.5, 'celeste-1');
         INSERT INTO games VALUES (2, 'Hades', 'hades', 'wine', 'Windows', NULL, 0, NULL, NULL, 'hades-2');
         INSERT INTO games VALUES (3, 'Portal', 'portal', 'steam', 'Linux', NULL, 1, NULL, 1.0, 'portal-3');",
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_db_and_art() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".local/share/lutris");
        std::fs::create_dir_all(root.join("coverart")).unwrap();
        make_test_db(&root.join("pga.db"));
        std::fs::write(root.join("coverart/celeste.jpg"), b"x").unwrap();

        let rows = read_db(&root.join("pga.db")).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].playtime_hours, Some(2.5));

        let games = scan(home.path());
        assert_eq!(games.len(), 2, "steam-runner rows are skipped");
        let celeste = &games[0];
        assert_eq!(celeste.id, "lutris:1");
        assert_eq!(celeste.playtime_minutes, Some(150));
        assert_eq!(celeste.last_played, Some(1_712_000_000));
        assert!(celeste.cover.is_some());
        assert!(!games[1].installed);
    }

    #[test]
    fn broken_db_is_not_fatal() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".local/share/lutris");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("pga.db"), b"not sqlite").unwrap();
        assert!(scan(home.path()).is_empty());
    }
}
