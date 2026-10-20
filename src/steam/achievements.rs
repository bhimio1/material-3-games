//! Achievements for Steam games: read from Steam's local cache
//! (`appcache/stats/*.bin`, binary KeyValues), optionally refined with the
//! Steam Web API, plus global unlock percentages (no key needed).

use super::bvdf::{self, Value};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Achievement {
    pub api_name: String,
    pub name: String,
    pub description: String,
    pub hidden: bool,
    pub icon_url: Option<String>,
    pub icon_gray_url: Option<String>,
    pub unlocked: bool,
    /// Unix seconds.
    pub unlock_time: Option<i64>,
    pub global_percent: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Achievements {
    pub list: Vec<Achievement>,
}

impl Achievements {
    pub fn unlocked(&self) -> usize {
        self.list.iter().filter(|a| a.unlocked).count()
    }

    pub fn total(&self) -> usize {
        self.list.len()
    }

    pub fn progress(&self) -> f32 {
        if self.list.is_empty() {
            0.0
        } else {
            self.unlocked() as f32 / self.list.len() as f32
        }
    }

    /// Unlocked first (newest first), then locked by global rarity.
    pub fn sort(&mut self) {
        self.list.sort_by(|a, b| {
            b.unlocked
                .cmp(&a.unlocked)
                .then(b.unlock_time.unwrap_or(0).cmp(&a.unlock_time.unwrap_or(0)))
                .then(
                    b.global_percent
                        .unwrap_or(0.0)
                        .partial_cmp(&a.global_percent.unwrap_or(0.0))
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });
    }
}

pub fn icon_url(appid: &str, icon: &str) -> Option<String> {
    if icon.is_empty() {
        return None;
    }
    if icon.starts_with("http") {
        return Some(icon.to_string());
    }
    Some(format!("https://steamcdn-a.akamaihd.net/steamcommunity/public/images/apps/{appid}/{icon}"))
}

/// A display string that may be a plain string or a map of languages.
fn localized(v: Option<&Value>) -> String {
    match v {
        Some(Value::Map(_)) => {
            let v = v.expect("matched Some");
            v.get_str("english")
                .or_else(|| v.entries().iter().find_map(|(k, x)| (k != "token").then(|| x.as_str()).flatten()))
                .unwrap_or_default()
        }
        Some(other) => other.as_str().unwrap_or_default(),
        None => String::new(),
    }
}

/// Parse `UserGameStatsSchema_<appid>.bin`. Returns achievements (all
/// locked) keyed by (stat id, bit).
pub fn parse_schema(data: &[u8], appid: &str) -> Vec<(String, u32, Achievement)> {
    let Ok(doc) = bvdf::parse(data) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // Root is "<appid>" (or occasionally anything); find its "stats".
    for (_, app) in doc.entries() {
        let Some(stats) = app.get("stats") else {
            continue;
        };
        for (stat_id, stat) in stats.entries() {
            let Some(bits) = stat.get("bits") else {
                continue;
            };
            for (bit_key, bit) in bits.entries() {
                let bit_no = bit.get_int("bit").or_else(|| bit_key.parse().ok()).unwrap_or(0) as u32;
                let display = bit.get("display");
                let api_name = bit.get_str("name").unwrap_or_default();
                let name = display.map(|d| localized(d.get("name"))).filter(|n| !n.is_empty()).unwrap_or_else(|| api_name.clone());
                let hidden = display.and_then(|d| d.get_int("hidden")).unwrap_or(0) != 0;
                let icon = display.and_then(|d| d.get_str("icon")).unwrap_or_default();
                let icon_gray = display.and_then(|d| d.get_str("icon_gray")).unwrap_or_default();
                out.push((
                    stat_id.clone(),
                    bit_no,
                    Achievement {
                        api_name,
                        name,
                        description: display.map(|d| localized(d.get("desc"))).unwrap_or_default(),
                        hidden,
                        icon_url: icon_url(appid, &icon),
                        icon_gray_url: icon_url(appid, &icon_gray),
                        ..Default::default()
                    },
                ));
            }
        }
    }
    out
}

/// Parse `UserGameStats_<account>_<appid>.bin`: stat id → (bit field,
/// unlock time per bit).
pub fn parse_user_stats(data: &[u8]) -> HashMap<String, (u64, HashMap<u32, i64>)> {
    let mut out = HashMap::new();
    let Ok(doc) = bvdf::parse(data) else {
        return out;
    };
    fn walk(v: &Value, out: &mut HashMap<String, (u64, HashMap<u32, i64>)>) {
        for (k, child) in v.entries() {
            if let Value::Map(_) = child {
                if let Some(bits) = child.get_int("data") {
                    let mut times = HashMap::new();
                    if let Some(t) = child.get("AchievementTimes") {
                        for (bit, time) in t.entries() {
                            if let (Ok(b), Some(ts)) = (bit.parse::<u32>(), time.as_int()) {
                                times.insert(b, ts);
                            }
                        }
                    }
                    out.insert(k.clone(), (bits as u64 & 0xFFFF_FFFF, times));
                } else {
                    walk(child, out);
                }
            }
        }
    }
    walk(&doc, &mut out);
    out
}

/// Achievements from Steam's local cache.
pub fn from_cache(root: &Path, account: Option<u32>, appid: &str) -> Option<Achievements> {
    let stats_dir = root.join("appcache/stats");
    let schema = std::fs::read(stats_dir.join(format!("UserGameStatsSchema_{appid}.bin"))).ok()?;
    let entries = parse_schema(&schema, appid);
    if entries.is_empty() {
        return None;
    }
    let user = account
        .and_then(|a| std::fs::read(stats_dir.join(format!("UserGameStats_{a}_{appid}.bin"))).ok())
        .map(|d| parse_user_stats(&d))
        .unwrap_or_default();
    let mut list = Vec::new();
    for (stat, bit, mut a) in entries {
        if let Some((bits, times)) = user.get(&stat) {
            a.unlocked = bit < 64 && bits & (1u64 << bit) != 0;
            a.unlock_time = times.get(&bit).copied().filter(|t| *t > 0);
        }
        list.push(a);
    }
    let mut a = Achievements { list };
    a.sort();
    Some(a)
}

#[derive(Deserialize)]
struct GlobalResp {
    achievementpercentages: GlobalList,
}

#[derive(Deserialize)]
struct GlobalList {
    #[serde(default)]
    achievements: Vec<GlobalItem>,
}

#[derive(Deserialize)]
struct GlobalItem {
    name: String,
    percent: serde_json::Value,
}

/// Parse `GetGlobalAchievementPercentagesForApp` (percent may be a number
/// or a string).
pub fn parse_global(json: &str) -> HashMap<String, f32> {
    serde_json::from_str::<GlobalResp>(json)
        .map(|r| {
            r.achievementpercentages
                .achievements
                .into_iter()
                .filter_map(|a| {
                    let p = match &a.percent {
                        serde_json::Value::Number(n) => n.as_f64()? as f32,
                        serde_json::Value::String(s) => s.parse().ok()?,
                        _ => return None,
                    };
                    Some((a.name, p))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn apply_global(a: &mut Achievements, global: &HashMap<String, f32>) {
    for x in &mut a.list {
        x.global_percent = global.get(&x.api_name).copied();
    }
    a.sort();
}

/// Merge Web API results (schema + player achievements) into a list.
pub fn parse_web(appid: &str, schema_json: &str, player_json: &str) -> Option<Achievements> {
    let schema: serde_json::Value = serde_json::from_str(schema_json).ok()?;
    let player: serde_json::Value = serde_json::from_str(player_json).unwrap_or_default();
    let defs = schema.pointer("/game/availableGameStats/achievements")?.as_array()?;
    let mut status: HashMap<String, (bool, i64)> = HashMap::new();
    if let Some(list) = player.pointer("/playerstats/achievements").and_then(|v| v.as_array()) {
        for a in list {
            if let Some(n) = a.get("apiname").and_then(|v| v.as_str()) {
                let achieved = a.get("achieved").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
                let t = a.get("unlocktime").and_then(|v| v.as_i64()).unwrap_or(0);
                status.insert(n.to_string(), (achieved, t));
            }
        }
    }
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let list = defs
        .iter()
        .map(|d| {
            let api_name = s(d, "name");
            let (unlocked, t) = status.get(&api_name).copied().unwrap_or((false, 0));
            Achievement {
                name: Some(s(d, "displayName")).filter(|n| !n.is_empty()).unwrap_or_else(|| api_name.clone()),
                description: s(d, "description"),
                hidden: d.get("hidden").and_then(|v| v.as_i64()).unwrap_or(0) != 0,
                icon_url: icon_url(appid, &s(d, "icon")),
                icon_gray_url: icon_url(appid, &s(d, "icongray")),
                unlocked,
                unlock_time: (t > 0).then_some(t),
                global_percent: None,
                api_name,
            }
        })
        .collect();
    let mut a = Achievements { list };
    a.sort();
    Some(a)
}

/// Everything we can find for `appid`: Web API when configured, else the
/// local cache; global percentages either way. Blocking (network).
pub fn load(root: Option<&Path>, account: Option<u32>, appid: &str, key: &str, steamid: &str) -> Option<Achievements> {
    let mut result = None;
    if !key.is_empty() && !steamid.is_empty() {
        let schema = super::web::get(&format!(
            "https://api.steampowered.com/ISteamUserStats/GetSchemaForGame/v2/?key={key}&appid={appid}&l=english"
        ));
        let player = super::web::get(&format!(
            "https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v1/?key={key}&steamid={steamid}&appid={appid}"
        ));
        if let (Ok(s), p) = (schema, player) {
            result = parse_web(appid, &s, &p.unwrap_or_default());
        }
    }
    if result.is_none() {
        result = root.and_then(|r| from_cache(r, account, appid));
    }
    let mut a = result?;
    if let Ok(g) = super::web::get(&format!(
        "https://api.steampowered.com/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/?gameid={appid}"
    )) {
        apply_global(&mut a, &parse_global(&g));
    }
    Some(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bvdf::Value as V;

    fn schema_fixture() -> Vec<u8> {
        let ach = |name: &str, bit: i32, title: &str, hidden: &str| {
            V::Map(vec![
                ("name".into(), V::str(name)),
                ("bit".into(), V::Int(bit)),
                (
                    "display".into(),
                    V::Map(vec![
                        ("name".into(), V::Map(vec![("english".into(), V::str(title)), ("token".into(), V::str("#x"))])),
                        ("desc".into(), V::Map(vec![("english".into(), V::str(&format!("{title} desc")))])),
                        ("hidden".into(), V::str(hidden)),
                        ("icon".into(), V::str(&format!("{name}.jpg"))),
                        ("icon_gray".into(), V::str(&format!("{name}_g.jpg"))),
                    ]),
                ),
            ])
        };
        let doc = V::Map(vec![(
            "620".into(),
            V::Map(vec![
                ("gamename".into(), V::str("Portal 2")),
                (
                    "stats".into(),
                    V::Map(vec![
                        (
                            "1".into(),
                            V::Map(vec![
                                ("type".into(), V::str("4")),
                                ("bits".into(), V::Map(vec![("0".into(), ach("ACH_A", 0, "Wake Up", "0")), ("1".into(), ach("ACH_B", 1, "Secret", "1"))])),
                            ]),
                        ),
                        ("2".into(), V::Map(vec![("type".into(), V::str("1")), ("name".into(), V::str("kills"))])),
                    ]),
                ),
            ]),
        )]);
        bvdf::write(&doc)
    }

    fn user_fixture(bits: i32) -> Vec<u8> {
        bvdf::write(&V::Map(vec![(
            "cache".into(),
            V::Map(vec![
                ("crc".into(), V::Int(123)),
                ("1".into(), V::Map(vec![("data".into(), V::Int(bits)), ("AchievementTimes".into(), V::Map(vec![("0".into(), V::Int(1_700_000_000))]))])),
            ]),
        )]))
    }

    #[test]
    fn local_cache() {
        let root = tempfile::tempdir().unwrap();
        let stats = root.path().join("appcache/stats");
        std::fs::create_dir_all(&stats).unwrap();
        std::fs::write(stats.join("UserGameStatsSchema_620.bin"), schema_fixture()).unwrap();
        std::fs::write(stats.join("UserGameStats_42_620.bin"), user_fixture(0b01)).unwrap();
        let a = from_cache(root.path(), Some(42), "620").unwrap();
        assert_eq!(a.total(), 2);
        assert_eq!(a.unlocked(), 1);
        let first = &a.list[0];
        assert_eq!(first.name, "Wake Up");
        assert_eq!(first.description, "Wake Up desc");
        assert_eq!(first.unlock_time, Some(1_700_000_000));
        assert_eq!(first.icon_url.as_deref(), Some("https://steamcdn-a.akamaihd.net/steamcommunity/public/images/apps/620/ACH_A.jpg"));
        assert!(a.list[1].hidden && !a.list[1].unlocked);
        // No user file: all locked.
        assert_eq!(from_cache(root.path(), Some(7), "620").unwrap().unlocked(), 0);
        assert!(from_cache(root.path(), Some(42), "1").is_none());
        assert!(parse_schema(b"\x00broken", "1").is_empty());
    }

    #[test]
    fn web_and_global() {
        let schema = r#"{"game":{"availableGameStats":{"achievements":[
            {"name":"A","displayName":"First","description":"d","hidden":0,"icon":"https://x/a.jpg","icongray":"https://x/ag.jpg"},
            {"name":"B","displayName":"","hidden":1,"icon":"","icongray":""}]}}}"#;
        let player = r#"{"playerstats":{"achievements":[{"apiname":"B","achieved":1,"unlocktime":5}]}}"#;
        let mut a = parse_web("1", schema, player).unwrap();
        assert_eq!(a.list[0].api_name, "B");
        assert_eq!(a.list[0].name, "B");
        assert!(a.list[0].unlocked);
        let g = parse_global(r#"{"achievementpercentages":{"achievements":[{"name":"A","percent":"12.5"},{"name":"B","percent":3.25}]}}"#);
        assert_eq!(g["A"], 12.5);
        apply_global(&mut a, &g);
        assert_eq!(a.list[1].global_percent, Some(12.5));
        assert!(parse_global("nope").is_empty());
        assert!(parse_web("1", "{}", "").is_none());
    }
}
