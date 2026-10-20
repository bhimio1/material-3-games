//! Non-Steam shortcuts in `userdata/<id>/config/shortcuts.vdf`, so Steam
//! launches our games and applies Steam Input (and its overlay) to them.
//!
//! Each shortcut runs `m3-games __run <game id>`: m3-games starts the game
//! with its per-game options (gamemode, MangoHud, env) and stays alive until
//! the game exits, so Steam keeps Steam Input active for the whole session.
//! Entries we own carry the tag "Material 3 Games"; other shortcuts are
//! never touched.

use super::bvdf::{self, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const TAG: &str = "Material 3 Games";

/// Steam's shortcut app id: CRC-32 of the quoted exe followed by the name,
/// with the top bit set (the scheme Steam ROM Manager and BoilR use).
pub fn shortcut_appid(exe_quoted: &str, name: &str) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(exe_quoted.as_bytes());
    h.update(name.as_bytes());
    h.finalize() | 0x8000_0000
}

/// The 64-bit id `steam://rungameid/` takes for a shortcut.
pub fn rungameid(appid: u32) -> u64 {
    ((appid as u64) << 32) | 0x0200_0000
}

pub fn quoted(path: &str) -> String {
    format!("\"{path}\"")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub game_id: String,
    pub name: String,
    /// Unquoted path of the program Steam runs.
    pub exe: String,
    pub start_dir: String,
    pub launch_options: String,
    pub icon: Option<PathBuf>,
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
}

impl Spec {
    /// The shortcut for a game in our library, run through m3-games.
    pub fn for_game(game: &crate::library::Game, m3_games: &Path) -> Spec {
        let exe = m3_games.to_string_lossy().into_owned();
        Spec {
            game_id: game.id.clone(),
            name: game.name.clone(),
            start_dir: m3_games.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            exe,
            launch_options: format!("__run {}", game.id),
            icon: game.cover.clone(),
            cover: game.cover.clone(),
            hero: game.hero.clone(),
        }
    }

    pub fn appid(&self) -> u32 {
        shortcut_appid(&quoted(&self.exe), &self.name)
    }
}

pub fn path(user_dir: &Path) -> PathBuf {
    user_dir.join("config/shortcuts.vdf")
}

/// Read shortcuts.vdf (an empty document when it doesn't exist).
pub fn read(file: &Path) -> Result<Value, String> {
    match std::fs::read(file) {
        Ok(data) => bvdf::parse(&data).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Map(vec![("shortcuts".into(), Value::Map(vec![]))])),
        Err(e) => Err(e.to_string()),
    }
}

fn shortcuts_mut(doc: &mut Value) -> &mut Vec<(String, Value)> {
    if doc.get("shortcuts").is_none() {
        doc.set("shortcuts", Value::Map(vec![]));
    }
    match doc.get_mut("shortcuts") {
        Some(Value::Map(e)) => e,
        _ => unreachable!("just inserted a map"),
    }
}

fn is_ours(entry: &Value) -> bool {
    entry.get("tags").is_some_and(|t| t.entries().iter().any(|(_, v)| v.as_str().as_deref() == Some(TAG)))
}

/// Game id → shortcut app id for the shortcuts we created.
pub fn ours(doc: &Value) -> HashMap<String, u32> {
    doc.get("shortcuts")
        .map(|s| s.entries())
        .unwrap_or(&[])
        .iter()
        .filter(|(_, e)| is_ours(e))
        .filter_map(|(_, e)| {
            let opts = e.get_str("LaunchOptions")?;
            let id = opts.strip_prefix("__run ")?.trim().to_string();
            Some((id, e.get_int("appid")? as i32 as u32))
        })
        .collect()
}

fn entry(spec: &Spec, appid: u32, previous: Option<&Value>) -> Value {
    let mut e = previous.cloned().unwrap_or(Value::Map(vec![]));
    e.set("appid", Value::Int(appid as i32));
    e.set("AppName", Value::str(&spec.name));
    e.set("Exe", Value::str(&quoted(&spec.exe)));
    e.set("StartDir", Value::str(&quoted(&spec.start_dir)));
    e.set("icon", Value::str(&spec.icon.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()));
    e.set("LaunchOptions", Value::str(&spec.launch_options));
    for (key, default) in [
        ("ShortcutPath", Value::str("")),
        ("IsHidden", Value::Int(0)),
        ("AllowDesktopConfig", Value::Int(1)),
        ("AllowOverlay", Value::Int(1)),
        ("OpenVR", Value::Int(0)),
        ("Devkit", Value::Int(0)),
        ("DevkitGameID", Value::str("")),
        ("DevkitOverrideAppID", Value::Int(0)),
        ("LastPlayTime", Value::Int(0)),
        ("FlatpakAppID", Value::str("")),
    ] {
        if e.get(key).is_none() {
            e.set(key, default);
        }
    }
    let mut tags: Vec<(String, Value)> = e.get("tags").map(|t| t.entries().to_vec()).unwrap_or_default();
    if !tags.iter().any(|(_, v)| v.as_str().as_deref() == Some(TAG)) {
        tags.push((tags.len().to_string(), Value::str(TAG)));
    }
    e.set("tags", Value::Map(tags));
    e
}

/// Add or update the shortcut for `spec`. Returns its app id.
pub fn upsert(doc: &mut Value, spec: &Spec) -> u32 {
    let appid = spec.appid();
    let list = shortcuts_mut(doc);
    let pos = list.iter().position(|(_, e)| {
        e.get_int("appid").is_some_and(|a| a as i32 as u32 == appid)
            || (is_ours(e) && e.get_str("LaunchOptions").as_deref() == Some(spec.launch_options.as_str()))
    });
    match pos {
        Some(i) => {
            let new = entry(spec, appid, Some(&list[i].1));
            list[i].1 = new;
        }
        None => {
            let key = list.len().to_string();
            list.push((key, entry(spec, appid, None)));
        }
    }
    appid
}

/// Remove our shortcut for `game_id`. Keys are renumbered as Steam expects.
pub fn remove(doc: &mut Value, game_id: &str) -> bool {
    let list = shortcuts_mut(doc);
    let before = list.len();
    let want = format!("__run {game_id}");
    list.retain(|(_, e)| !(is_ours(e) && e.get_str("LaunchOptions").as_deref() == Some(want.as_str())));
    for (i, (k, _)) in list.iter_mut().enumerate() {
        *k = i.to_string();
    }
    list.len() != before
}

fn copy_art(src: &Option<PathBuf>, grid: &Path, stem: &str) {
    if let Some(src) = src.as_ref().filter(|p| p.is_file()) {
        let ext = src.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "jpg".into());
        let _ = std::fs::create_dir_all(grid);
        let _ = std::fs::copy(src, grid.join(format!("{stem}.{ext}")));
    }
}

/// Grid art Steam shows for a shortcut: portrait cover, hero and the wide
/// capsule (the hero if there is one, else the cover).
pub fn write_grid_art(user_dir: &Path, appid: u32, spec: &Spec) {
    let grid = user_dir.join("config/grid");
    copy_art(&spec.cover, &grid, &format!("{appid}p"));
    copy_art(&spec.hero, &grid, &format!("{appid}_hero"));
    copy_art(if spec.hero.is_some() { &spec.hero } else { &spec.cover }, &grid, &appid.to_string());
}

/// Add/update `add` and remove `remove` in one write, with a backup.
/// Steam must be closed (see [`super::account::with_steam_closed`]).
pub fn apply(user_dir: &Path, add: &[Spec], remove_ids: &[String]) -> Result<HashMap<String, u32>, String> {
    let file = path(user_dir);
    let mut doc = read(&file)?;
    let mut ids = HashMap::new();
    for spec in add {
        let appid = upsert(&mut doc, spec);
        write_grid_art(user_dir, appid, spec);
        ids.insert(spec.game_id.clone(), appid);
    }
    for id in remove_ids {
        remove(&mut doc, id);
    }
    super::account::backup(&file, 10).map_err(|e| e.to_string())?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = file.with_extension("vdf.m3-tmp");
    std::fs::write(&tmp, bvdf::write(&doc)).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    Ok(ids)
}

/// Shortcut app ids we created, read from the current user's file.
pub fn current(home: &Path) -> HashMap<String, u32> {
    super::account::root(home)
        .and_then(|r| super::account::user_dir(&r))
        .and_then(|u| read(&path(&u)).ok())
        .map(|d| ours(&d))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain bitwise CRC-32 (IEEE), independent of crc32fast.
    fn reference_crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    #[test]
    fn appid_math() {
        assert_eq!(reference_crc32(b"123456789"), 0xCBF4_3926);
        let exe = "\"/usr/bin/m3-games\"";
        let id = shortcut_appid(exe, "Celeste");
        assert_eq!(id, reference_crc32(format!("{exe}Celeste").as_bytes()) | 0x8000_0000);
        assert!(id & 0x8000_0000 != 0);
        assert_eq!(rungameid(0x8000_0001), 0x8000_0001_0200_0000);
    }

    fn spec(id: &str, name: &str) -> Spec {
        Spec {
            game_id: id.into(),
            name: name.into(),
            exe: "/usr/bin/m3-games".into(),
            start_dir: "/usr/bin".into(),
            launch_options: format!("__run {id}"),
            icon: None,
            cover: None,
            hero: None,
        }
    }

    #[test]
    fn preserves_other_shortcuts() {
        let mut doc = bvdf::parse(&super::super::bvdf::tests_fixture()).unwrap();
        let a = upsert(&mut doc, &spec("lutris:3", "Celeste"));
        let b = upsert(&mut doc, &spec("manual:x", "X"));
        assert_ne!(a, b);
        // Updating keeps one entry per game.
        upsert(&mut doc, &spec("lutris:3", "Celeste"));
        let list = doc.get("shortcuts").unwrap().entries();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].1.get_str("AppName").as_deref(), Some("Quake"));
        assert_eq!(list[0].1.get_int("UnknownFuture"), Some(42));
        let ours = ours(&doc);
        assert_eq!(ours.len(), 2);
        assert_eq!(ours["lutris:3"], a);
        assert_eq!(list[1].1.get_str("Exe").as_deref(), Some("\"/usr/bin/m3-games\""));
        assert_eq!(list[1].1.get_int("AllowDesktopConfig"), Some(1));

        // Round trip through bytes.
        let back = bvdf::parse(&bvdf::write(&doc)).unwrap();
        assert_eq!(back, doc);

        assert!(remove(&mut doc, "lutris:3"));
        assert!(!remove(&mut doc, "nope"));
        let keys: Vec<_> = doc.get("shortcuts").unwrap().entries().iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, ["0", "1"]);
    }

    #[test]
    fn apply_writes_file_backup_and_art() {
        let user = tempfile::tempdir().unwrap();
        let cover = user.path().join("cover.png");
        std::fs::write(&cover, b"img").unwrap();
        let mut s = spec("manual:x", "X");
        s.cover = Some(cover);
        let ids = apply(user.path(), &[s.clone()], &[]).unwrap();
        let appid = ids["manual:x"];
        let doc = read(&path(user.path())).unwrap();
        assert_eq!(ours(&doc)["manual:x"], appid);
        assert!(user.path().join(format!("config/grid/{appid}p.png")).is_file());
        assert!(user.path().join(format!("config/grid/{appid}.png")).is_file());
        // Second write makes a backup of the first.
        apply(user.path(), &[], &["manual:x".into()]).unwrap();
        let backups = std::fs::read_dir(user.path().join("config")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".bak")).count();
        assert_eq!(backups, 1);
        assert!(ours(&read(&path(user.path())).unwrap()).is_empty());
    }
}
