//! Patch notes and announcements from `ISteamNews/GetNewsForApp` (no key).
//!
//! Posts come as BBCode (Steam's own announcements) or HTML (outside feeds),
//! and are turned into the guide reader's blocks so they render the same way.

use super::guides::{self, Block};
use serde::Deserialize;
use std::time::Duration;

/// Where `{STEAM_CLAN_IMAGE}` and `{STEAM_CLAN_LOC_IMAGE}` point.
pub const CLAN_IMAGES: &str = "https://clan.akamai.steamstatic.com/images";

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
    /// The post laid out for the reader.
    #[serde(skip)]
    pub blocks: Vec<Block>,
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

/// The news URL. `official` limits it to the developer's own announcements;
/// otherwise outside feeds (gaming sites, in any language) are mixed in.
pub fn url(appid: &str, official: bool) -> String {
    let feeds = if official { "&feeds=steam_community_announcements" } else { "" };
    format!("https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid={appid}&count=10&maxlength=0&format=json{feeds}")
}

pub fn parse(json: &str) -> Vec<NewsItem> {
    serde_json::from_str::<Resp>(json)
        .map(|r| r.appnews.newsitems.into_iter().map(|mut n| {
            n.blocks = guides::parse_fragment(&to_html(&n.contents));
            n.contents = render(&n.contents);
            n
        }).collect())
        .unwrap_or_default()
}

fn expand_clan_images(s: &str) -> String {
    s.replace("{STEAM_CLAN_IMAGE}", CLAN_IMAGES).replace("{STEAM_CLAN_LOC_IMAGE}", CLAN_IMAGES)
}

fn attr_escape(s: &str) -> String {
    s.trim().trim_matches(['"', '\'']).replace('"', "&quot;").replace('<', "&lt;")
}

/// BBCode tags that start or end a block: newlines next to them are layout,
/// not line breaks.
fn is_block_tag(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "p" | "list" | "olist" | "*" | "quote" | "table" | "tr" | "td" | "th" | "hr" | "img" | "previewyoutube" | "code")
}

/// A Steam news post as HTML. BBCode is converted (unknown tags are left as
/// text); HTML is passed through. Clan image placeholders are expanded and
/// YouTube previews become a linked thumbnail.
pub fn to_html(src: &str) -> String {
    let src = expand_clan_images(src);
    let html_input = ["<p", "<br", "<img", "<a ", "<li", "<div", "<h2", "<h3"].iter().any(|t| src.contains(t));
    let mut out = String::with_capacity(src.len() + src.len() / 4);
    let lower = src.to_ascii_lowercase();
    let mut i = 0;
    // The text between two tags, with its newlines.
    let mut pending = String::new();
    let mut last_block = true;
    let flush_text = |pending: &mut String, out: &mut String, last_block: bool, next_block: bool| {
        let mut t = pending.as_str();
        if !html_input {
            if last_block {
                t = t.trim_start_matches(['\n', '\r', ' ']);
            }
            if next_block {
                t = t.trim_end_matches(['\n', '\r', ' ']);
            }
            out.push_str(&t.replace('<', "&lt;").replace('>', "&gt;").replace('\n', "<br>"));
        } else {
            out.push_str(t);
        }
        pending.clear();
    };
    while i < src.len() {
        let c = src.as_bytes()[i];
        if c == b'[' {
            if let Some(end) = src[i..].find(']').filter(|e| *e < 300) {
                let inner = &src[i + 1..i + end];
                let closing = inner.starts_with('/');
                let body = inner.trim_start_matches('/');
                let (name, arg) = match body.find(['=', ' ']) {
                    Some(k) => (&body[..k], Some(&body[k + 1..])),
                    None => (body, None),
                };
                let name = name.to_ascii_lowercase();
                let after = i + end + 1;
                // The text up to `[/name]`, and where the closing tag ends.
                let content_to = |tag: &str| -> Option<(String, usize)> {
                    let close = format!("[/{tag}]");
                    lower[after..].find(&close).map(|j| (src[after..after + j].to_string(), after + j + close.len()))
                };
                let tag: Option<(String, usize)> = match (name.as_str(), closing) {
                    ("h1" | "h2" | "h3", false) => Some((format!("<{name}>"), after)),
                    ("h1" | "h2" | "h3", true) => Some((format!("</{name}>"), after)),
                    ("b" | "i" | "u" | "s" | "p", false) => Some((format!("<{}>", if name == "s" { "del" } else { &name }), after)),
                    ("b" | "i" | "u" | "s" | "p", true) => Some((format!("</{}>", if name == "s" { "del" } else { &name }), after)),
                    ("strike" | "spoiler" | "noparse", _) => Some((String::new(), after)),
                    ("list", false) => Some(("<ul>".into(), after)),
                    ("list", true) => Some(("</ul>".into(), after)),
                    ("olist", false) => Some(("<ol>".into(), after)),
                    ("olist", true) => Some(("</ol>".into(), after)),
                    ("*", false) => Some(("<li>".into(), after)),
                    ("*", true) => Some(("</li>".into(), after)),
                    ("quote", false) => Some(("<blockquote>".into(), after)),
                    ("quote", true) => Some(("</blockquote>".into(), after)),
                    ("code", false) => Some(("<p>".into(), after)),
                    ("code", true) => Some(("</p>".into(), after)),
                    ("table" | "tr" | "td" | "th", false) => Some((format!("<{name}>"), after)),
                    ("table" | "tr" | "td" | "th", true) => Some((format!("</{name}>"), after)),
                    ("hr", false) => Some(("<hr>".into(), after)),
                    ("hr", true) => Some((String::new(), after)),
                    ("url", false) => match arg {
                        Some(href) => Some((format!("<a href=\"{}\">", attr_escape(href)), after)),
                        None => content_to("url").map(|(href, end)| {
                            let href = attr_escape(&href);
                            (format!("<a href=\"{href}\">{href}</a>"), end)
                        }),
                    },
                    ("url", true) => Some(("</a>".into(), after)),
                    ("dynamiclink", false) => {
                        let href = arg.and_then(|a| a.split_once("href=")).map(|(_, h)| attr_escape(h.split_whitespace().next().unwrap_or("")));
                        let end = content_to("dynamiclink").map(|(_, e)| e).unwrap_or(after);
                        href.filter(|h| !h.is_empty()).map(|h| (format!("<p><a href=\"{h}\">{h}</a></p>"), end)).or(Some((String::new(), end)))
                    }
                    ("img", false) => {
                        // [img]url[/img] or [img src="url"][/img]
                        let from_attr = arg.and_then(|a| a.split_once("src=").map(|(_, v)| v).or(Some(a)))
                            .map(|v| attr_escape(v.split_whitespace().next().unwrap_or("")))
                            .filter(|v| v.starts_with("http"));
                        match content_to("img") {
                            Some((body, end)) => {
                                let url = from_attr.unwrap_or_else(|| attr_escape(&body));
                                Some((format!("<p><img src=\"{url}\"></p>"), end))
                            }
                            None => from_attr.map(|u| (format!("<p><img src=\"{u}\"></p>"), after)),
                        }
                    }
                    ("previewyoutube", false) => {
                        let id: String = arg.unwrap_or("").split(';').next().unwrap_or("").chars()
                            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
                        let end = content_to("previewyoutube").map(|(_, e)| e).unwrap_or(after);
                        if id.is_empty() {
                            Some((String::new(), end))
                        } else {
                            let watch = format!("https://www.youtube.com/watch?v={id}");
                            Some((format!("<p><a href=\"{watch}\"><img src=\"https://img.youtube.com/vi/{id}/hqdefault.jpg\"></a></p><p><a href=\"{watch}\">Watch on YouTube</a></p>"), end))
                        }
                    }
                    ("video", false) => Some((String::new(), content_to("video").map(|(_, e)| e).unwrap_or(after))),
                    _ => None,
                };
                if let Some((html, next)) = tag {
                    let block = is_block_tag(&name);
                    flush_text(&mut pending, &mut out, last_block, block);
                    out.push_str(&html);
                    last_block = block;
                    i = next;
                    continue;
                }
            }
        }
        let ch = src[i..].chars().next().unwrap_or(' ');
        pending.push(ch);
        i += ch.len_utf8();
    }
    flush_text(&mut pending, &mut out, last_block, true);
    out
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

/// The developer's announcements, or all feeds when it has posted none
/// (cached for 6 hours; offline from cache).
pub fn fetch(appid: &str) -> Result<Vec<NewsItem>, String> {
    let get = |official: bool| -> Result<Vec<NewsItem>, String> {
        let key = format!("{appid}-{}", if official { "official" } else { "all" });
        let json = super::web::get_cached("news", &key, &url(appid, official), Duration::from_secs(6 * 3600))?;
        Ok(parse(&json))
    };
    let official = get(true)?;
    if official.is_empty() { get(false) } else { Ok(official) }
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
#[cfg(test)]
mod real_posts {
    use super::*;
    use crate::steam::guides::plain;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/steam/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn mixed_feeds() {
        let news = parse(&fixture("news-570.json"));
        assert_eq!(news.len(), 10);
        // An outside feed's HTML post.
        let pg = &news[0];
        assert!(pg.feedlabel.starts_with("PlayGround.ru"));
        assert!(matches!(&pg.blocks[0], Block::Image { url, .. } if url.starts_with("https://i.playground.ru/")));
        assert!(pg.blocks.iter().filter(|b| matches!(b, Block::Paragraph(_))).count() >= 3);
        // [p]-style BBCode with a localized clan image.
        let trove = &news[1];
        assert_eq!(trove.title, "Inside the Trove of Terror");
        assert_eq!(trove.blocks[0], Block::Image {
            url: "https://clan.akamai.steamstatic.com/images/3703047/44e04da47da8b634dee235756e4ffa1234879b40.png".into(),
            thumb: false,
            link: None,
        });
        // Patch notes: a link and a list.
        let patch = &news[2];
        assert!(matches!(&patch.blocks[0], Block::Paragraph(s) if s.iter().any(|s| s.text == "here" && s.link.is_some())));
        assert!(patch.blocks.iter().filter(|b| matches!(b, Block::Item(_))).count() > 5);
        // Headings.
        assert!(news.iter().any(|n| n.blocks.contains(&Block::Heading(2, "Attending The International".into()))));
        // Plain text stays for compact views.
        assert!(!trove.contents.contains('['));
    }

    #[test]
    fn terraria_state_of_the_game() {
        let n = &parse(&fixture("news-105600-official.json"))[0];
        assert!(n.blocks.contains(&Block::Heading(1, "PC - RE-LOGIC".into())));
        assert!(n.blocks.iter().all(|b| !format!("{b:?}").contains("STEAM_CLAN")));
        // [previewyoutube] becomes a linked thumbnail and a link.
        let yt = n.blocks.iter().find_map(|b| match b {
            Block::Image { url, link, .. } if url.contains("img.youtube.com") => link.clone(),
            _ => None,
        });
        assert!(yt.unwrap().starts_with("https://www.youtube.com/watch?v="));
        assert!(n.blocks.iter().any(|b| matches!(b, Block::Paragraph(s) if plain(s) == "Watch on YouTube")));
        assert!(n.blocks.iter().any(|b| matches!(b, Block::Item(_))));
        // No BBCode left over as text.
        for b in &n.blocks {
            if let Block::Paragraph(s) | Block::Item(s) = b {
                let t = plain(s);
                assert!(!t.contains("[/") && !t.contains("[b]"), "{t}");
            }
        }
    }

    #[test]
    fn bbcode() {
        let html = to_html("[h2]Fixes[/h2]\n[list]\n[*]Crash on [b]start[/b]\n[*]Audio & video\n[/list]\nLine one\nline two [url=\"https://x.y/z\"]link[/url]\n[img]{STEAM_CLAN_IMAGE}/1/a.png[/img]a < b");
        let b = guides::parse_fragment(&html);
        assert_eq!(b[0], Block::Heading(2, "Fixes".into()));
        assert!(matches!(&b[1], Block::Item(s) if plain(s) == "Crash on start" && s[1].bold));
        assert!(matches!(&b[2], Block::Item(s) if plain(s) == "Audio & video"));
        let Block::Paragraph(p) = &b[3] else { panic!("{:?}", b[3]) };
        assert_eq!(plain(p), "Line one\nline two link");
        assert_eq!(p.last().unwrap().link.as_deref(), Some("https://x.y/z"));
        assert_eq!(b[4], Block::Image { url: format!("{CLAN_IMAGES}/1/a.png"), thumb: false, link: None });
        assert!(matches!(&b[5], Block::Paragraph(s) if plain(s) == "a < b"));
        // Unknown tags stay as text.
        assert!(to_html("[unknown] [tag").contains("[unknown] [tag"));
    }

    #[test]
    fn feeds() {
        assert!(url("620", true).ends_with("&feeds=steam_community_announcements"));
        assert!(!url("620", false).contains("feeds="));
    }
}
