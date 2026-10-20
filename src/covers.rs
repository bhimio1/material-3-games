//! Cover art: a disk cache (`~/.cache/m3-games/covers`) filled from Heroic's
//! store art and, with an API key, SteamGridDB.

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

/// Cached cover for a game id, if downloaded before.
pub fn cached(id: &str) -> Option<PathBuf> {
    let p = dir().join(format!("{}.img", key(id)));
    p.is_file().then_some(p)
}

/// Fill in covers from the cache for games that have none.
pub fn apply_cached(games: &mut [Game]) {
    for g in games.iter_mut().filter(|g| g.cover.is_none()) {
        g.cover = cached(&g.id);
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(20)).build()
}

fn download(agent: &ureq::Agent, url: &str, id: &str) -> Option<PathBuf> {
    let resp = agent.get(url).call().ok()?;
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

/// Fetch covers for games without one. Returns how many were found.
pub fn fetch_missing(games: &[Game], api_key: &str) -> usize {
    let agent = agent();
    let mut found = 0;
    for g in games.iter().filter(|g| g.cover.is_none()) {
        let url = g.cover_url.clone().or_else(|| {
            (!api_key.is_empty()).then(|| sgdb_cover(&agent, api_key, g)).flatten()
        });
        if let Some(url) = url {
            if download(&agent, &url, &g.id).is_some() {
                found += 1;
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
    }
}
