//! Steam: library folders, app manifests, cover art from the library cache,
//! and last-played / play time from `localconfig.vdf`.

use super::vdf::{self, Vdf};
use super::{Game, Launch, Source};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Steam installs we know about (native and Flatpak), deduplicated.
pub fn roots(home: &Path) -> Vec<PathBuf> {
    let candidates = [
        home.join(".local/share/Steam"),
        home.join(".steam/steam"),
        home.join(".steam/root"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
    ];
    let mut out: Vec<PathBuf> = Vec::new();
    for c in candidates {
        if !c.join("steamapps").is_dir() {
            continue;
        }
        let real = std::fs::canonicalize(&c).unwrap_or(c);
        if !out.contains(&real) {
            out.push(real);
        }
    }
    out
}

/// Tools and runtimes that show up as apps but aren't games.
pub fn is_tool(name: &str) -> bool {
    const PREFIXES: [&str; 5] = [
        "Proton",
        "Steam Linux Runtime",
        "Steamworks Common Redistributables",
        "Steamworks SDK",
        "SteamVR",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Library folders listed in `libraryfolders.vdf`, plus the root itself.
pub fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let text = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
    let doc = vdf::parse(&text);
    if let Some(lf) = doc.get("libraryfolders") {
        for (_, folder) in lf.entries() {
            if let Some(p) = folder.get_str("path") {
                let p = PathBuf::from(p);
                let real = std::fs::canonicalize(&p).unwrap_or(p);
                if !out.contains(&real) {
                    out.push(real);
                }
            }
        }
    }
    out
}

#[derive(Debug, PartialEq)]
pub struct Manifest {
    pub appid: String,
    pub name: String,
    pub installed: bool,
    pub install_dir: Option<String>,
    pub last_played: Option<i64>,
}

pub fn parse_manifest(text: &str) -> Option<Manifest> {
    let doc = vdf::parse(text);
    let state = doc.get("AppState")?;
    let appid = state.get_str("appid")?.to_string();
    let name = state.get_str("name").unwrap_or(&appid).to_string();
    let flags: u64 = state.get_str("StateFlags").and_then(|f| f.parse().ok()).unwrap_or(0);
    Some(Manifest {
        appid,
        name,
        // 4 = fully installed.
        installed: flags & 4 != 0,
        install_dir: state.get_str("installdir").map(str::to_string),
        last_played: state.get_str("LastPlayed").and_then(|t| t.parse().ok()).filter(|t| *t > 0),
    })
}

/// LastPlayed and Playtime (minutes) per appid from every user's
/// `localconfig.vdf`.
pub fn local_stats(root: &Path) -> HashMap<String, (Option<i64>, Option<u64>)> {
    let mut out: HashMap<String, (Option<i64>, Option<u64>)> = HashMap::new();
    let Ok(users) = std::fs::read_dir(root.join("userdata")) else {
        return out;
    };
    for user in users.flatten() {
        let Ok(text) = std::fs::read_to_string(user.path().join("config/localconfig.vdf")) else {
            continue;
        };
        merge_local_stats(&vdf::parse(&text), &mut out);
    }
    out
}

pub fn merge_local_stats(doc: &Vdf, out: &mut HashMap<String, (Option<i64>, Option<u64>)>) {
    let Some(apps) = doc.path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]) else {
        return;
    };
    for (appid, app) in apps.entries() {
        let last = app.get_str("LastPlayed").and_then(|t| t.parse::<i64>().ok()).filter(|t| *t > 0);
        let play = app.get_str("Playtime").and_then(|t| t.parse::<u64>().ok());
        let e = out.entry(appid.clone()).or_default();
        e.0 = e.0.max(last);
        e.1 = match (e.1, play) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }
}

/// Find an image in the library cache, in either the old flat layout
/// (`<appid>_library_600x900.jpg`) or the new per-app folders
/// (`<appid>/library_600x900.jpg`, sometimes one level deeper).
pub fn cached_image(root: &Path, appid: &str, names: &[&str]) -> Option<PathBuf> {
    let cache = root.join("appcache/librarycache");
    for name in names {
        let flat = cache.join(format!("{appid}_{name}"));
        if flat.is_file() {
            return Some(flat);
        }
        let dir = cache.join(appid);
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path().join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn scan(home: &Path) -> Vec<Game> {
    let mut games = Vec::new();
    for root in roots(home) {
        let stats = local_stats(&root);
        for lib in library_folders(&root) {
            let steamapps = lib.join("steamapps");
            let Ok(entries) = std::fs::read_dir(&steamapps) else {
                continue;
            };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                    continue;
                }
                let Some(m) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_manifest(&t)) else {
                    continue;
                };
                if is_tool(&m.name) || m.appid == "228980" {
                    continue;
                }
                let (last, play) = stats.get(&m.appid).copied().unwrap_or((None, None));
                games.push(Game {
                    id: format!("steam:{}", m.appid),
                    cover: cached_image(&root, &m.appid, &["library_600x900.jpg", "library_capsule.jpg", "header.jpg"]),
                    hero: cached_image(&root, &m.appid, &["library_hero.jpg"]),
                    cover_url: None,
                    last_played: last.max(m.last_played),
                    playtime_minutes: play,
                    installed: m.installed,
                    install_dir: m.install_dir.as_ref().map(|d| steamapps.join("common").join(d)),
                    launch: Launch::Steam { appid: m.appid.clone() },
                    name: m.name,
                    source: Source::Steam,
                });
            }
        }
    }
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const ACF: &str = r#""AppState"
{
	"appid"		"570"
	"Universe"		"1"
	"name"		"Dota 2"
	"StateFlags"		"4"
	"installdir"		"dota 2 beta"
	"LastPlayed"		"1700000000"
}"#;

    #[test]
    fn manifest() {
        let m = parse_manifest(ACF).unwrap();
        assert_eq!(m.appid, "570");
        assert_eq!(m.name, "Dota 2");
        assert!(m.installed);
        assert_eq!(m.install_dir.as_deref(), Some("dota 2 beta"));
        assert_eq!(m.last_played, Some(1_700_000_000));
        let downloading = ACF.replace("\"StateFlags\"\t\t\"4\"", "\"StateFlags\"\t\t\"1026\"");
        assert!(!parse_manifest(&downloading).unwrap().installed);
        assert!(parse_manifest("garbage").is_none());
    }

    #[test]
    fn tools_are_skipped() {
        assert!(is_tool("Proton 9.0"));
        assert!(is_tool("Steam Linux Runtime 3.0 (sniper)"));
        assert!(!is_tool("Portal 2"));
    }

    #[test]
    fn localconfig_stats() {
        let doc = vdf::parse(
            r#""UserLocalConfigStore" { "Software" { "valve" { "Steam" { "apps" {
                "570" { "LastPlayed" "1710000000" "Playtime" "1234" }
                "620" { "Playtime" "5" }
            } } } } }"#,
        );
        let mut out = HashMap::new();
        merge_local_stats(&doc, &mut out);
        assert_eq!(out["570"], (Some(1_710_000_000), Some(1234)));
        assert_eq!(out["620"], (None, Some(5)));
    }

    #[test]
    fn full_scan() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".local/share/Steam");
        let apps = root.join("steamapps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("appmanifest_570.acf"), ACF).unwrap();
        std::fs::write(
            apps.join("appmanifest_1493710.acf"),
            "\"AppState\" { \"appid\" \"1493710\" \"name\" \"Proton Experimental\" \"StateFlags\" \"4\" }",
        )
        .unwrap();
        let cache = root.join("appcache/librarycache/570/abc123");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("library_600x900.jpg"), b"x").unwrap();
        std::fs::write(root.join("appcache/librarycache/570_library_hero.jpg"), b"x").unwrap();

        let games = scan(home.path());
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.id, "steam:570");
        assert!(g.cover.as_ref().unwrap().ends_with("abc123/library_600x900.jpg"));
        assert!(g.hero.as_ref().unwrap().ends_with("570_library_hero.jpg"));
        assert_eq!(g.class_hint().as_deref(), Some("steam_app_570"));
    }
}
