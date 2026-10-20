//! Patch notes and announcements from `ISteamNews/GetNewsForApp` (no key).

use serde::Deserialize;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NewsItem {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub contents: String,
    #[serde(default)]
    pub feedlabel: String,
    #[serde(default)]
    pub date: i64,
}

#[derive(Deserialize)]
struct Resp {
    appnews: AppNews,
}

#[derive(Deserialize)]
struct AppNews {
    #[serde(default)]
    newsitems: Vec<NewsItem>,
}

pub fn url(appid: &str) -> String {
    format!("https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid={appid}&count=10&maxlength=0&format=json")
}

pub fn parse(json: &str) -> Vec<NewsItem> {
    serde_json::from_str::<Resp>(json)
        .map(|r| r.appnews.newsitems.into_iter().map(|mut n| {
            n.contents = render(&n.contents);
            n
        }).collect())
        .unwrap_or_default()
}

/// Turn Steam's BBCode (and the odd bit of HTML) into readable text:
/// headings and list items on their own lines, tags dropped.
pub fn render(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let b = src.as_bytes();
    while i < b.len() {
        let c = b[i];
        if c == b'[' || c == b'<' {
            let close = if c == b'[' { b']' } else { b'>' };
            if let Some(end) = b[i..].iter().position(|x| *x == close).filter(|e| *e < 200) {
                let tag = src[i + 1..i + end].trim().to_ascii_lowercase();
                let name = tag.trim_start_matches('/').split(['=', ' ']).next().unwrap_or("");
                let closing = tag.starts_with('/');
                match name {
                    "h1" | "h2" | "h3" | "p" | "list" | "olist" | "table" | "tr" | "hr" | "div" => {
                        if !out.ends_with('\n') && !out.is_empty() {
                            out.push('\n');
                        }
                        if !closing && matches!(name, "h1" | "h2" | "h3") && !out.ends_with("\n\n") && !out.is_empty() {
                            out.push('\n');
                        }
                    }
                    "*" | "li" => {
                        if !closing {
                            if !out.ends_with('\n') && !out.is_empty() {
                                out.push('\n');
                            }
                            out.push_str("• ");
                        }
                    }
                    "br" => out.push('\n'),
                    "img" | "previewyoutube" | "video" if !closing => {
                        // Skip the element's contents up to its closing tag.
                        let closer = if c == b'[' { format!("[/{name}]") } else { String::new() };
                        if !closer.is_empty() {
                            if let Some(j) = src[i..].to_ascii_lowercase().find(&closer) {
                                i += j + closer.len();
                                continue;
                            }
                        }
                    }
                    _ => {}
                }
                i += end + 1;
                continue;
            }
        }
        let ch = src[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    // Squeeze runs of blank lines and trim each line.
    let mut lines: Vec<&str> = Vec::new();
    for l in out.lines().map(str::trim) {
        if l.is_empty() && lines.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        lines.push(l);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n").replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">")
}

/// Fetch (cached for 6 hours; offline from cache).
pub fn fetch(appid: &str) -> Result<Vec<NewsItem>, String> {
    let json = super::web::get_cached("news", appid, &url(appid), Duration::from_secs(6 * 3600))?;
    Ok(parse(&json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_news() {
        let json = r#"{"appnews":{"appid":620,"newsitems":[{"gid":"1","title":"Update 1.2","url":"https://x","is_external_url":false,"author":"Valve","contents":"[h2]Fixes[/h2][list][*]Crash on start[*]Audio &amp; video[/list][img]{STEAM_CLAN_IMAGE}/a.png[/img]Thanks!","feedlabel":"Community Announcements","date":1700000000}],"count":1}}"#;
        let n = parse(json);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].title, "Update 1.2");
        assert_eq!(n[0].contents, "Fixes\n• Crash on start\n• Audio & video\nThanks!");
        assert!(parse("{}").is_empty());
    }

    #[test]
    fn html_and_url_tags() {
        assert_eq!(render("<p>Hello <b>world</b></p><ul><li>a</li><li>b</li></ul>"), "Hello world\n• a\n• b");
        assert_eq!(render("[url=https://x]link[/url] text [unknown"), "link text [unknown");
    }
}
