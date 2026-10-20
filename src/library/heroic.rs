//! Heroic Games Launcher: installed Epic (legendary), GOG and Amazon (nile)
//! games. Titles and cover URLs come from Heroic's store caches.

use super::{Game, Launch, Source};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn roots(home: &Path) -> Vec<PathBuf> {
    [
        home.join(".config/heroic"),
        home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic"),
    ]
    .into_iter()
    .filter(|p| p.is_dir())
    .collect()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// app name → (title, cover url) from a `store_cache/*_library.json`.
pub fn library_titles(doc: &Value) -> HashMap<String, (String, Option<String>)> {
    let list = doc
        .get("games")
        .or_else(|| doc.get("library"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.iter()
        .filter_map(|g| {
            let id = g.get("app_name").or_else(|| g.get("appName"))?.as_str()?.to_string();
            let title = g.get("title")?.as_str()?.to_string();
            let art = ["art_square", "art_cover"]
                .iter()
                .find_map(|k| g.get(*k).and_then(Value::as_str))
                .filter(|s| s.starts_with("http"))
                .map(str::to_string);
            Some((id, (title, art)))
        })
        .collect()
}

#[derive(Debug, PartialEq)]
pub struct Installed {
    pub runner: &'static str,
    pub app_name: String,
    pub title: Option<String>,
    pub path: Option<String>,
}

/// legendary `installed.json`: an object keyed by app name.
pub fn parse_legendary(doc: &Value) -> Vec<Installed> {
    doc.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| Installed {
                    runner: "legendary",
                    app_name: v.get("app_name").and_then(Value::as_str).unwrap_or(k).to_string(),
                    title: v.get("title").and_then(Value::as_str).map(str::to_string),
                    path: v.get("install_path").and_then(Value::as_str).map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// gog_store `installed.json`: `{"installed": [{appName, install_path}]}`.
pub fn parse_gog(doc: &Value) -> Vec<Installed> {
    doc.get("installed")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| {
                    Some(Installed {
                        runner: "gog",
                        app_name: v.get("appName")?.as_str()?.to_string(),
                        title: None,
                        path: v.get("install_path").and_then(Value::as_str).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// nile `installed.json`: `[{id, path}]`.
pub fn parse_nile(doc: &Value) -> Vec<Installed> {
    doc.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| {
                    Some(Installed {
                        runner: "nile",
                        app_name: v.get("id")?.as_str()?.to_string(),
                        title: None,
                        path: v.get("path").and_then(Value::as_str).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn scan(home: &Path) -> Vec<Game> {
    let mut games = Vec::new();
    for root in roots(home) {
        let mut titles = HashMap::new();
        for lib in ["legendary_library.json", "gog_library.json", "nile_library.json"] {
            if let Some(doc) = read_json(&root.join("store_cache").join(lib)) {
                titles.extend(library_titles(&doc));
            }
        }
        let mut installed = Vec::new();
        if let Some(doc) = read_json(&root.join("legendaryConfig/legendary/installed.json")) {
            installed.extend(parse_legendary(&doc));
        }
        if let Some(doc) = read_json(&root.join("gog_store/installed.json")) {
            installed.extend(parse_gog(&doc));
        }
        if let Some(doc) = read_json(&root.join("nile_config/nile/installed.json")) {
            installed.extend(parse_nile(&doc));
        }
        for i in installed {
            let (lib_title, art) = titles.get(&i.app_name).cloned().unzip();
            let name = i.title.clone().or(lib_title).unwrap_or_else(|| i.app_name.clone());
            games.push(Game {
                id: format!("heroic:{}:{}", i.runner, i.app_name),
                name,
                source: Source::Heroic,
                cover: None,
                hero: None,
                cover_url: art.flatten(),
                last_played: None,
                playtime_minutes: None,
                installed: true,
                shortcut_appid: None,
                install_dir: i.path.map(PathBuf::from),
                launch: Launch::Heroic {
                    runner: i.runner.to_string(),
                    app_name: i.app_name,
                },
            });
        }
    }
    games
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_all_stores() {
        let leg = parse_legendary(&json!({"Fortnite": {"app_name": "Fortnite", "title": "Fortnite", "install_path": "/g/fn"}}));
        assert_eq!(leg[0].title.as_deref(), Some("Fortnite"));
        let gog = parse_gog(&json!({"installed": [{"appName": "1207658924", "install_path": "/g/w"}, {"bad": 1}]}));
        assert_eq!(gog.len(), 1);
        let nile = parse_nile(&json!([{"id": "amzn1.x", "path": "/g/a"}]));
        assert_eq!(nile[0].runner, "nile");
        assert!(parse_gog(&json!("nonsense")).is_empty());
    }

    #[test]
    fn scan_with_titles() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".config/heroic");
        std::fs::create_dir_all(root.join("gog_store")).unwrap();
        std::fs::create_dir_all(root.join("store_cache")).unwrap();
        std::fs::write(root.join("gog_store/installed.json"), r#"{"installed":[{"appName":"1207658924","install_path":"/g/w"}]}"#).unwrap();
        std::fs::write(
            root.join("store_cache/gog_library.json"),
            r#"{"games":[{"app_name":"1207658924","title":"The Witcher","art_square":"https://img/x.jpg"}]}"#,
        )
        .unwrap();
        let games = scan(home.path());
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, "heroic:gog:1207658924");
        assert_eq!(games[0].name, "The Witcher");
        assert_eq!(games[0].cover_url.as_deref(), Some("https://img/x.jpg"));
    }
}
