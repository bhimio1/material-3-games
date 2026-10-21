//! Games the account owns but hasn't installed (needs a Web API key).

use crate::library::{Game, Launch, Source};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
struct Resp {
    response: Inner,
}

#[derive(Deserialize)]
struct Inner {
    #[serde(default)]
    games: Vec<Owned>,
}

#[derive(Deserialize)]
struct Owned {
    appid: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    playtime_forever: u64,
    #[serde(default)]
    rtime_last_played: i64,
}

pub fn parse(json: &str) -> Vec<Game> {
    serde_json::from_str::<Resp>(json)
        .map(|r| {
            r.response
                .games
                .into_iter()
                .filter(|g| !g.name.is_empty())
                .map(|g| Game {
                    id: format!("steam:{}", g.appid),
                    name: g.name,
                    source: Source::Steam,
                    cover: None,
                    hero: None,
                    cover_url: Some(crate::covers::steam_art_url(&g.appid.to_string(), "library_600x900.jpg")),
                    last_played: (g.rtime_last_played > 0).then_some(g.rtime_last_played),
                    playtime_minutes: (g.playtime_forever > 0).then_some(g.playtime_forever),
                    installed: false,
                    shortcut_appid: None,
                    install_dir: None,
                    launch: Launch::Steam { appid: g.appid.to_string() },
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Owned games (cached for 12 hours, offline from cache). Blocking.
pub fn fetch(key: &str, steamid: &str) -> Result<Vec<Game>, String> {
    if key.is_empty() || steamid.is_empty() {
        return Err("Set a Steam Web API key first.".into());
    }
    let url = format!("https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?key={key}&steamid={steamid}&include_appinfo=1&include_played_free_games=1&format=json");
    let json = super::web::get_cached("owned", steamid, &url, Duration::from_secs(12 * 3600))?;
    Ok(parse(&json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_games() {
        let g = parse(r#"{"response":{"game_count":2,"games":[{"appid":620,"name":"Portal 2","playtime_forever":600,"rtime_last_played":1700000000},{"appid":1,"name":""}]}}"#);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].id, "steam:620");
        assert!(!g[0].installed);
        assert_eq!(g[0].playtime_minutes, Some(600));
        assert!(parse("{}").is_empty());
    }
}
