//! Cover art: a disk cache (`~/.cache/m3-games/covers`) filled from Steam's
//! CDN (Steam games whose art Steam hasn't cached locally), Heroic's store
//! art and, with an API key, SteamGridDB.

use crate::library::{cache_dir, Game, Launch};
use serde_json::Value;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::PathBuf;

pub fn dir() -> PathBuf {
    cache_dir().join("covers")
}

fn key(id: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Cache key of a game's hero (wide banner) art.
fn hero_id(id: &str) -> String {
    format!("{id}#hero")
}

/// Steam's public art for an app: `library_600x900.jpg` (cover),
/// `library_hero.jpg`, `header.jpg`, `logo.png`.
pub fn steam_art_url(appid: &str, file: &str) -> String {
    format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{appid}/{file}")
}

/// Where to download a cover for `game` from, without an API key.
fn remote_cover(game: &Game) -> Option<String> {
    game.cover_url.clone().or_else(|| game.steam_appid().map(|a| steam_art_url(a, "library_600x900.jpg")))
}

/// Whether [`fetch_missing`] has anything to look for.
pub fn wants_fetch(game: &Game, api_key: &str) -> bool {
    (game.cover.is_none() && (remote_cover(game).is_some() || !api_key.is_empty())) || (game.hero.is_none() && game.steam_appid().is_some())
}

/// Cached cover for a game id, if downloaded before.
pub fn cached(id: &str) -> Option<PathBuf> {
    let p = dir().join(format!("{}.img", key(id)));
    p.is_file().then_some(p)
}

/// Fill in covers and heroes from the cache for games that have none.
pub fn apply_cached(games: &mut [Game]) {
    for g in games.iter_mut() {
        if g.cover.is_none() {
            g.cover = cached(&g.id);
        }
        if g.hero.is_none() {
            g.hero = cached(&hero_id(&g.id));
        }
    }
}

/// Marks art the server doesn't have, so it isn't asked for again for a week.
fn missing_marker(id: &str) -> PathBuf {
    dir().join(format!("{}.missing", key(id)))
}

fn download(agent: &ureq::Agent, url: &str, id: &str) -> Option<PathBuf> {
    let marker = missing_marker(id);
    let recent = std::fs::metadata(&marker).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if recent.is_some_and(|age| age < std::time::Duration::from_secs(7 * 24 * 3600)) {
        return None;
    }
    let resp = match agent.get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => {
            let _ = std::fs::create_dir_all(dir());
            let _ = std::fs::write(&marker, url);
            return None;
        }
        Err(_) => return None,
    };
    let mut bytes = Vec::new();
    resp.into_reader().take(16 << 20).read_to_end(&mut bytes).ok()?;
    image::guess_format(&bytes).ok()?;
    let path = dir().join(format!("{}.img", key(id)));
    std::fs::create_dir_all(dir()).ok()?;
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

/// First grid URL in a SteamGridDB response.
pub fn first_url(doc: &Value) -> Option<String> {
    doc.get("data")?.as_array()?.iter().find_map(|g| g.get("url")?.as_str().map(str::to_string))
}

fn sgdb(agent: &ureq::Agent, key: &str, path: &str) -> Option<Value> {
    agent
        .get(&format!("https://www.steamgriddb.com/api/v2/{path}"))
        .set("Authorization", &format!("Bearer {key}"))
        .call()
        .ok()?
        .into_json()
        .ok()
}

fn sgdb_cover(agent: &ureq::Agent, api_key: &str, game: &Game) -> Option<String> {
    let dims = "dimensions=600x900,342x482,660x930";
    if let Launch::Steam { appid } = &game.launch {
        if let Some(u) = sgdb(agent, api_key, &format!("grids/steam/{appid}?{dims}")).as_ref().and_then(first_url) {
            return Some(u);
        }
    }
    let term: String = game.name.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let search = sgdb(agent, api_key, &format!("search/autocomplete/{}", term.split_whitespace().collect::<Vec<_>>().join("%20")))?;
    let gid = search.get("data")?.as_array()?.first()?.get("id")?.as_i64()?;
    sgdb(agent, api_key, &format!("grids/game/{gid}?{dims}")).as_ref().and_then(first_url)
}

/// Fetch covers (and Steam heroes) for games without one. Returns how many
/// were found.
pub fn fetch_missing(games: &[Game], api_key: &str) -> usize {
    let agent = crate::steam::web::agent();
    let mut found = 0;
    for g in games {
        if g.cover.is_none() {
            let mut got = remote_cover(g).is_some_and(|url| download(&agent, &url, &g.id).is_some());
            if !got && !api_key.is_empty() {
                got = sgdb_cover(&agent, api_key, g).is_some_and(|url| download(&agent, &url, &g.id).is_some());
            }
            found += got as usize;
        }
        if g.hero.is_none() {
            if let Some(appid) = g.steam_appid() {
                found += download(&agent, &steam_art_url(appid, "library_hero.jpg"), &hero_id(&g.id)).is_some() as usize;
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgdb_response() {
        let doc = serde_json::json!({"success": true, "data": [{"id": 1, "url": "https://cdn2.steamgriddb.com/grid/a.png"}]});
        assert_eq!(first_url(&doc).as_deref(), Some("https://cdn2.steamgriddb.com/grid/a.png"));
        assert_eq!(first_url(&serde_json::json!({"data": []})), None);
        assert_ne!(key("steam:1"), key("steam:2"));
        assert_ne!(key("steam:1"), key(&hero_id("steam:1")));
    }

    #[test]
    fn steam_fallback() {
        let mut g = crate::library::Game {
            id: "steam:620".into(),
            name: "Portal 2".into(),
            source: crate::library::Source::Steam,
            cover: None,
            hero: None,
            cover_url: None,
            last_played: None,
            playtime_minutes: None,
            installed: true,
            install_dir: None,
            launch: Launch::Steam { appid: "620".into() },
            shortcut_appid: None,
        };
        assert_eq!(remote_cover(&g).as_deref(), Some("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/library_600x900.jpg"));
        assert!(wants_fetch(&g, ""));
        g.cover = Some("/c.jpg".into());
        g.hero = Some("/h.jpg".into());
        assert!(!wants_fetch(&g, ""));
    }
}
