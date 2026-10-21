//! Steam Community guides: the list for a game and a native reader for a
//! guide page. Steam has no public API for guide content, so both are read
//! from the community HTML pages. The parsers follow the markup Steam serves
//! today (checked against live pages; fixtures in `tests/fixtures/steam/`)
//! and fall back gracefully when it changes.

use scraper::{ElementRef, Html, Node, Selector};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuideItem {
    pub id: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub preview: Option<String>,
    /// 0–5 stars when shown.
    pub stars: Option<u8>,
}

impl GuideItem {
    pub fn url(&self) -> String {
        guide_url(&self.id)
    }
}

/// One page of a game's guides (30 per page), optionally only guides tagged
/// with a Steam language (`english`, `german`, …).
pub fn list_url(appid: &str, page: u32, language: Option<&str>) -> String {
    let mut url = format!("https://steamcommunity.com/app/{appid}/guides/?browsefilter=toprated&p={}", page.max(1));
    if let Some(lang) = language.filter(|l| !l.is_empty()) {
        url.push_str(&format!("&requiredtags%5B%5D={lang}"));
    }
    url
}

/// Steam's guide-language tag for the system locale (`$LANG` etc.), or None
/// when it has no tag for it.
pub fn steam_language(locale: &str) -> Option<&'static str> {
    let l = locale.split(['.', '@']).next().unwrap_or("").to_ascii_lowercase();
    let (lang, region) = l.split_once(['_', '-']).unwrap_or((&l, ""));
    Some(match (lang, region) {
        ("en", _) => "english",
        ("de", _) => "german",
        ("fr", _) => "french",
        ("it", _) => "italian",
        ("es", "mx" | "ar" | "co" | "cl" | "pe" | "ve" | "us") => "latam",
        ("es", _) => "spanish",
        ("pt", "br") => "brazilian",
        ("pt", _) => "portuguese",
        ("ru", _) => "russian",
        ("pl", _) => "polish",
        ("uk", _) => "ukrainian",
        ("tr", _) => "turkish",
        ("nl", _) => "dutch",
        ("sv", _) => "swedish",
        ("da", _) => "danish",
        ("fi", _) => "finnish",
        ("no" | "nb" | "nn", _) => "norwegian",
        ("cs", _) => "czech",
        ("hu", _) => "hungarian",
        ("ro", _) => "romanian",
        ("el", _) => "greek",
        ("bg", _) => "bulgarian",
        ("th", _) => "thai",
        ("vi", _) => "vietnamese",
        ("ja", _) => "japanese",
        ("ko", _) => "koreana",
        ("zh", "tw" | "hk" | "mo") => "tchinese",
        ("zh", _) => "schinese",
        _ => return None,
    })
}

/// The guide language for this system.
pub fn system_language() -> Option<&'static str> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty() && s != "C" && s != "POSIX"))
        .and_then(|l| steam_language(&l))
}

pub fn guide_url(id: &str) -> String {
    format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

fn text_of(e: ElementRef) -> String {
    e.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn id_from_href(href: &str) -> Option<String> {
    let i = href.find("id=")?;
    let id: String = href[i + 3..].chars().take_while(|c| c.is_ascii_digit()).collect();
    (!id.is_empty()).then_some(id)
}

/// A page of guide results.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuidePage {
    pub items: Vec<GuideItem>,
    /// "Showing 1-30 of 1,662 entries" → 1662.
    pub total: Option<u32>,
    pub has_more: bool,
    /// Steam showed its mature-content check instead of the list.
    pub content_check: bool,
}

/// Parse a game's guide list page, with paging info.
pub fn parse_page(html: &str) -> GuidePage {
    let items = parse_list(html);
    let doc = Html::parse_document(html);
    let info = doc.select(&sel(".workshopBrowsePagingInfo")).next().map(text_of).unwrap_or_default();
    // "Showing 1-30 of 1,662 entries"
    let nums: Vec<u32> = info
        .split(|c: char| !(c.is_ascii_digit() || c == ','))
        .filter_map(|t| t.replace(',', "").parse().ok())
        .collect();
    let (end, total) = match nums.as_slice() {
        [_, end, total, ..] => (Some(*end), Some(*total)),
        _ => (None, None),
    };
    let has_more = match (end, total) {
        (Some(e), Some(t)) => e < t,
        _ => doc.select(&sel("a.pagebtn")).any(|a| text_of(a) == ">"),
    };
    let content_check = items.is_empty() && html.contains("wants_mature_content_apps");
    GuidePage { items, total, has_more, content_check }
}

/// Parse the items of a game's guide list page.
pub fn parse_list(html: &str) -> Vec<GuideItem> {
    let doc = Html::parse_document(html);
    let mut out: Vec<GuideItem> = Vec::new();
    let links = sel("a[href*='filedetails/?id=']");
    let title_sel = sel(".workshopItemTitle");
    let desc_sel = sel(".workshopItemShortDesc");
    let author_sel = sel(".workshopItemAuthorName");
    let preview_sel = sel("img.workshopItemPreviewImage");
    let any_img_sel = sel("img:not(.backgroundImg):not(.fileRating)");
    let rating_sel = sel("img.fileRating");
    for a in doc.select(&links) {
        let Some(id) = a.value().attr("href").and_then(id_from_href) else {
            continue;
        };
        if out.iter().any(|g| g.id == id) {
            continue;
        }
        // The item is the link's container when there is one (author and
        // rating sit beside the link), else the link itself.
        let container = a
            .ancestors()
            .take(3)
            .filter_map(ElementRef::wrap)
            .find(|e| e.value().classes().any(|c| c == "workshopItemCollectionContainer" || c == "workshopItem"))
            .unwrap_or(a);
        let Some(title) = container.select(&title_sel).next().map(text_of).filter(|t| !t.is_empty()) else {
            continue;
        };
        let author = container
            .select(&author_sel)
            .next()
            .map(text_of)
            .map(|a| a.trim_start_matches("by ").trim().to_string())
            .unwrap_or_default();
        let stars = container
            .select(&rating_sel)
            .next()
            .and_then(|i| i.value().attr("src"))
            .and_then(|src| src.rsplit('/').next())
            .and_then(|f| f.chars().next())
            .and_then(|c| c.to_digit(10))
            .map(|d| d as u8);
        out.push(GuideItem {
            id,
            title,
            author,
            description: container.select(&desc_sel).next().map(text_of).unwrap_or_default(),
            // The real preview, not the small blurred background copy.
            preview: container
                .select(&preview_sel)
                .next()
                .or_else(|| container.select(&any_img_sel).next())
                .and_then(|i| i.value().attr("src"))
                .map(str::to_string),
            stars,
        });
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(u8, String),
    Paragraph(Vec<Span>),
    Item(Vec<Span>),
    /// `thumb`: Steam's small floated thumbnails (`sizeThumb`), shown beside
    /// the text that follows rather than full width. `link`: where clicking
    /// it goes (the full-size image, or a video).
    Image { url: String, thumb: bool, link: Option<String> },
    /// Quotes can hold any blocks (Steam guides put headings and images in
    /// them).
    Quote(Vec<Block>),
    /// Rows of cells; `header` cells come from `<th>` / `bb_table_th`.
    Table(Vec<Vec<Cell>>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub header: bool,
    pub spans: Vec<Span>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guide {
    pub title: String,
    pub description: String,
    pub authors: String,
    pub blocks: Vec<Block>,
}

/// Steam wraps external links in its link filter.
/// Steam wraps external links in its link filter: `linkfilter/?u=<url>`
/// on current pages, `?url=` on older ones.
fn unwrap_link(href: &str) -> String {
    for marker in ["linkfilter/?u=", "linkfilter/?url="] {
        if let Some(i) = href.find(marker) {
            let rest = &href[i + marker.len()..];
            return percent_decode(rest.split('&').next().unwrap_or(rest));
        }
    }
    href.to_string()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
}

struct Builder {
    blocks: Vec<Block>,
    spans: Vec<Span>,
}

impl Builder {
    fn push_text(&mut self, text: &str, style: Style, link: Option<&str>) {
        if text.is_empty() {
            return;
        }
        let collapsed = collapse_ws(text);
        if collapsed.trim().is_empty() && self.spans.last().is_none_or(|s| s.text.ends_with([' ', '\n'])) {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            if last.bold == style.bold && last.italic == style.italic && last.link.as_deref() == link {
                last.text.push_str(&collapsed);
                return;
            }
        }
        self.spans.push(Span { text: collapsed, bold: style.bold, italic: style.italic, link: link.map(str::to_string) });
    }

    fn newline(&mut self) {
        match self.spans.last_mut() {
            Some(last) if last.text.ends_with("\n\n") => {}
            Some(last) => {
                // Two <br>s in a row end the paragraph.
                if last.text.ends_with('\n') {
                    self.flush();
                } else {
                    last.text = last.text.trim_end_matches(' ').to_string();
                    last.text.push('\n');
                }
            }
            None => {}
        }
    }

    fn flush(&mut self) {
        let mut spans = std::mem::take(&mut self.spans);
        if let Some(l) = spans.last_mut() {
            l.text = l.text.trim_end().to_string();
        }
        if let Some(f) = spans.first_mut() {
            f.text = f.text.trim_start().to_string();
        }
        spans.retain(|s| !s.text.is_empty());
        if !spans.is_empty() {
            self.blocks.push(Block::Paragraph(spans));
        }
    }
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn heading_level(e: &scraper::node::Element) -> Option<u8> {
    match e.name() {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" | "h4" | "h5" => Some(3),
        "div" => e.classes().find_map(|c| match c {
            "bb_h1" => Some(1),
            "bb_h2" => Some(2),
            "bb_h3" => Some(3),
            _ => None,
        }),
        _ => None,
    }
}

fn is_row(e: &scraper::node::Element) -> bool {
    e.name() == "tr" || (e.name() == "div" && e.classes().any(|c| c == "bb_table_tr"))
}

fn is_cell(e: &scraper::node::Element) -> Option<bool> {
    match e.name() {
        "td" => Some(false),
        "th" => Some(true),
        "div" => e.classes().find_map(|c| match c {
            "bb_table_td" => Some(false),
            "bb_table_th" => Some(true),
            _ => None,
        }),
        _ => None,
    }
}

/// Rows of a `<table>` or Steam's `div.bb_table` (rows may sit inside
/// `tbody`/`thead`).
fn table_rows(table: ElementRef, style: Style, link: Option<&str>) -> Vec<Vec<Cell>> {
    let mut rows = Vec::new();
    for node in table.descendants() {
        let Some(row) = ElementRef::wrap(node) else { continue };
        if !is_row(row.value()) {
            continue;
        }
        // Skip rows of nested tables: their closest table must be ours.
        let owner = row
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find(|a| a.value().name() == "table" || a.value().classes().any(|c| c == "bb_table"));
        if owner.map(|o| o.id()) != Some(table.id()) {
            continue;
        }
        let mut cells = Vec::new();
        for c in row.children().filter_map(ElementRef::wrap) {
            let Some(header) = is_cell(c.value()) else { continue };
            let mut inner = Builder { blocks: Vec::new(), spans: Vec::new() };
            walk(*c, &mut inner, Style { bold: header || style.bold, ..style }, link);
            let mut spans = Vec::new();
            inner.flush();
            for blk in inner.blocks {
                if let Block::Paragraph(s) | Block::Item(s) = blk {
                    if !spans.is_empty() {
                        spans.push(Span { text: "\n".into(), bold: false, italic: false, link: None });
                    }
                    spans.extend(s);
                }
            }
            cells.push(Cell { header, spans });
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
    }
    rows
}

fn walk(node: ego_tree::NodeRef<Node>, b: &mut Builder, style: Style, link: Option<&str>) {
    for child in node.children() {
        match child.value() {
            Node::Text(t) => b.push_text(&t.text, style, link),
            Node::Element(e) => {
                let el = ElementRef::wrap(child).expect("element node");
                if let Some(level) = heading_level(e) {
                    b.flush();
                    let t = text_of(el);
                    if !t.is_empty() {
                        b.blocks.push(Block::Heading(level, t));
                    }
                    continue;
                }
                match e.name() {
                    "br" => b.newline(),
                    "b" | "strong" => walk(child, b, Style { bold: true, ..style }, link),
                    "i" | "em" | "u" => walk(child, b, Style { italic: true, ..style }, link),
                    "a" => {
                        let href = e.attr("href").map(unwrap_link);
                        // Image links: just the image.
                        walk(child, b, style, href.as_deref().filter(|h| !h.is_empty()).or(link));
                    }
                    "img" => {
                        if let Some(src) = e.attr("src").filter(|s| s.starts_with("http")) {
                            let thumb = e.classes().any(|c| c == "sizeThumb");
                            b.flush();
                            b.blocks.push(Block::Image { url: src.to_string(), thumb, link: link.map(str::to_string) });
                        }
                    }
                    "li" => {
                        b.flush();
                        let mut inner = Builder { blocks: Vec::new(), spans: Vec::new() };
                        walk(child, &mut inner, style, link);
                        inner.flush();
                        for blk in inner.blocks {
                            match blk {
                                Block::Paragraph(s) => b.blocks.push(Block::Item(s)),
                                other => b.blocks.push(other),
                            }
                        }
                    }
                    "blockquote" => {
                        b.flush();
                        let mut inner = Builder { blocks: Vec::new(), spans: Vec::new() };
                        walk(child, &mut inner, style, link);
                        inner.flush();
                        if !inner.blocks.is_empty() {
                            b.blocks.push(Block::Quote(inner.blocks));
                        }
                    }
                    "table" => {
                        b.flush();
                        let rows = table_rows(el, style, link);
                        if !rows.is_empty() {
                            b.blocks.push(Block::Table(rows));
                        }
                    }
                    "div" if e.classes().any(|c| c == "bb_table") => {
                        b.flush();
                        let rows = table_rows(el, style, link);
                        if !rows.is_empty() {
                            b.blocks.push(Block::Table(rows));
                        }
                    }
                    "div" if e.classes().any(|c| c == "bb_blockquote" || c == "quote") => {
                        b.flush();
                        let mut inner = Builder { blocks: Vec::new(), spans: Vec::new() };
                        walk(child, &mut inner, style, link);
                        inner.flush();
                        if !inner.blocks.is_empty() {
                            b.blocks.push(Block::Quote(inner.blocks));
                        }
                    }
                    // Steam appends "[host]" after external links.
                    "span" if e.classes().any(|c| c == "bb_link_host") => {}
                    "ul" | "ol" | "p" | "div" | "tr" | "hr" => {
                        b.flush();
                        walk(child, b, style, link);
                        b.flush();
                    }
                    "script" | "style" => {}
                    _ => walk(child, b, style, link),
                }
            }
            _ => {}
        }
    }
}

/// Parse a guide page into readable blocks.
pub fn parse_guide(html: &str) -> Guide {
    let doc = Html::parse_document(html);
    let first_text = |s: &str| doc.select(&sel(s)).next().map(text_of).unwrap_or_default();
    let mut g = Guide {
        title: first_text(".workshopItemTitle"),
        description: first_text(".guideTopDescription"),
        authors: first_text(".guideAuthors, .friendBlockContent"),
        blocks: Vec::new(),
    };
    if g.title.is_empty() {
        g.title = first_text(".guideTopTitle");
    }
    if g.title.is_empty() {
        // "Steam Community :: Guide :: <title>"
        let t = first_text("title");
        g.title = t.rsplit(" :: ").next().unwrap_or(&t).to_string();
    }
    g.authors = g.authors.trim_start_matches("By ").to_string();
    let section = sel(".subSection");
    let title_sel = sel(".subSectionTitle");
    let desc_sel = sel(".subSectionDesc");
    let mut b = Builder { blocks: Vec::new(), spans: Vec::new() };
    for s in doc.select(&section) {
        if let Some(t) = s.select(&title_sel).next().map(text_of).filter(|t| !t.is_empty()) {
            b.flush();
            b.blocks.push(Block::Heading(1, t));
        }
        if let Some(d) = s.select(&desc_sel).next() {
            walk(*d, &mut b, Style::default(), None);
            b.flush();
        }
    }
    // Pages without sections (older layout): the description body.
    if b.blocks.is_empty() {
        if let Some(d) = doc.select(&sel(".workshopItemDescription, #highlightContent")).next() {
            walk(*d, &mut b, Style::default(), None);
            b.flush();
        }
    }
    g.blocks = b.blocks;
    g
}

/// Blocks for an HTML fragment (news posts reuse the guide reader).
pub fn parse_fragment(html: &str) -> Vec<Block> {
    let doc = Html::parse_fragment(html);
    let mut b = Builder { blocks: Vec::new(), spans: Vec::new() };
    walk(*doc.root_element(), &mut b, Style::default(), None);
    b.flush();
    b.blocks
}

/// Plain-text version of a span list.
#[cfg(test)]
pub fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

/// A page of guides for an app (cached for a day, offline from cache). With
/// a language, only guides tagged with it; when that finds nothing on the
/// first page, all languages instead (`language_used` says which).
pub fn fetch_list(appid: &str, page: u32, language: Option<&str>) -> Result<(GuidePage, Option<String>), String> {
    let get = |lang: Option<&str>| -> Result<GuidePage, String> {
        let key = format!("list-{appid}-p{page}-{}", lang.unwrap_or("all"));
        // Steam shows a content check for mature games; the cookie is what its
        // "Proceed" button sets, scoped to this app.
        let cookie = format!("wants_mature_content_apps={appid}");
        let html = super::web::get_cached_with_cookie("guides", &key, &list_url(appid, page, lang), &cookie, Duration::from_secs(24 * 3600))?;
        Ok(parse_page(&html))
    };
    let mut used = language.map(str::to_string);
    let mut result = get(language)?;
    if result.items.is_empty() && page <= 1 && language.is_some() {
        result = get(None)?;
        used = None;
    }
    if result.items.is_empty() {
        return Err(if result.content_check {
            "Steam wants a mature-content confirmation for this game's guides. Open them in the browser once.".into()
        } else {
            "No guides found (or Steam changed its page layout).".into()
        });
    }
    Ok((result, used))
}

/// One guide (cached for a week, offline from cache).
pub fn fetch_guide(id: &str) -> Result<Guide, String> {
    let html = super::web::get_cached("guides", &format!("guide-{id}"), &guide_url(id), Duration::from_secs(7 * 24 * 3600))?;
    let g = parse_guide(&html);
    if g.blocks.is_empty() && g.title.is_empty() {
        return Err("Couldn't read this guide.".into());
    }
    Ok(g)
}

/// A guide the user pinned from a local Markdown/text file.
pub fn from_markdown(title: &str, text: &str) -> Guide {
    let mut blocks = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let span = |t: &str| vec![Span { text: t.to_string(), bold: false, italic: false, link: None }];
    let paragraph = |t: &str| Block::Paragraph(span(t));
    let flush = |para: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if !para.is_empty() {
            blocks.push(Block::Paragraph(span(&para.join(" "))));
            para.clear();
        }
    };
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            flush(&mut para, &mut blocks);
        } else if let Some(h) = l.strip_prefix("### ") {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Heading(3, h.into()));
        } else if let Some(h) = l.strip_prefix("## ") {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Heading(2, h.into()));
        } else if let Some(h) = l.strip_prefix("# ") {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Heading(1, h.into()));
        } else if let Some(i) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Item(span(i)));
        } else if let Some(q) = l.strip_prefix("> ") {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Quote(vec![paragraph(q)]));
        } else if l.starts_with("![") {
            flush(&mut para, &mut blocks);
            if let Some(url) = l.split_once("](").map(|(_, r)| r.trim_end_matches(')')) {
                blocks.push(Block::Image { url: url.to_string(), thumb: false, link: None });
            }
        } else {
            para.push(l.to_string());
        }
    }
    flush(&mut para, &mut blocks);
    Guide { title: title.into(), description: String::new(), authors: String::new(), blocks }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"<html><body><div class="workshopBrowseItems">
<div class="workshopItemCollectionContainer">
 <a href="https://steamcommunity.com/sharedfiles/filedetails/?id=1234567890&searchtext=" class="workshopItemCollection ui-draggable">
  <div class="workshopItemPreviewHolder"><img class="workshopItemPreviewImage" src="https://steamuserimages-a.akamaihd.net/ugc/1/preview.jpg"></div>
  <div class="workshopItemDetails">
   <div class="workshopItemTitle">100% Achievement   Guide</div>
   <div class="workshopItemShortDesc">Everything, step by step.</div>
  </div>
 </a>
 <div class="workshopItemAuthorName">by&nbsp;<a href="https://steamcommunity.com/id/someone/">Someone</a></div>
 <img class="fileRating" src="https://community.akamai.steamstatic.com/public/images/sharedfiles/5-star_large.png">
</div>
<div class="workshopItemCollectionContainer">
 <a href="https://steamcommunity.com/sharedfiles/filedetails/?id=42" class="workshopItemCollection">
  <div class="workshopItemDetails"><div class="workshopItemTitle">Speedrun Routes</div></div>
 </a>
</div>
<a href="https://steamcommunity.com/sharedfiles/filedetails/?id=42">duplicate link</a>
</div></body></html>"#;

    #[test]
    fn guide_list() {
        let list = parse_list(LIST);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "1234567890");
        assert_eq!(list[0].title, "100% Achievement Guide");
        assert_eq!(list[0].description, "Everything, step by step.");
        assert_eq!(list[0].author, "Someone");
        assert_eq!(list[0].stars, Some(5));
        assert_eq!(list[0].preview.as_deref(), Some("https://steamuserimages-a.akamaihd.net/ugc/1/preview.jpg"));
        assert_eq!(list[1].title, "Speedrun Routes");
        assert!(parse_list("<html>nothing</html>").is_empty());
    }

    const GUIDE: &str = r#"<html><body>
<div class="guideTopTitle">  The Complete Guide </div>
<div class="guideTopDescription">All secrets.</div>
<div class="guide subSections">
 <div class="subSection detailBox">
  <div class="subSectionTitle">Getting started</div>
  <div class="subSectionDesc">First line<br>second <b>bold</b> and <i>italic</i>.<br><br>New paragraph with a
   <a class="bb_link" href="https://steamcommunity.com/linkfilter/?url=https%3A%2F%2Fexample.com%2Fmap" target="_blank">map link</a>.
   <div class="bb_h2">Controls</div>
   <ul class="bb_ul"><li>Jump: A</li><li><b>Dash</b>: X</li></ul>
   <a class="modalContentLink" href="https://x/full.jpg"><img src="https://steamuserimages-a.akamaihd.net/ugc/2/shot.jpg"></a>
   <blockquote class="bb_blockquote">Quoted tip</blockquote>
  </div>
 </div>
 <div class="subSection detailBox"><div class="subSectionTitle">Chapter 2</div><div class="subSectionDesc">Go left.</div></div>
</div></body></html>"#;

    #[test]
    fn guide_page() {
        let g = parse_guide(GUIDE);
        assert_eq!(g.title, "The Complete Guide");
        assert_eq!(g.description, "All secrets.");
        let b = &g.blocks;
        assert_eq!(b[0], Block::Heading(1, "Getting started".into()));
        let Block::Paragraph(p1) = &b[1] else { panic!("{:?}", b[1]) };
        assert_eq!(plain(p1), "First line\nsecond bold and italic.");
        assert!(p1.iter().any(|s| s.bold && s.text == "bold"));
        assert!(p1.iter().any(|s| s.italic && s.text == "italic"));
        let Block::Paragraph(p2) = &b[2] else { panic!("{:?}", b[2]) };
        let link = p2.iter().find(|s| s.link.is_some()).unwrap();
        assert_eq!(link.text, "map link");
        assert_eq!(link.link.as_deref(), Some("https://example.com/map"));
        assert!(b.contains(&Block::Heading(2, "Controls".into())));
        assert!(b.iter().any(|x| matches!(x, Block::Item(s) if plain(s) == "Jump: A")));
        assert!(b.iter().any(|x| matches!(x, Block::Item(s) if plain(s) == "Dash: X" && s[0].bold)));
        assert!(b.contains(&Block::Image {
            url: "https://steamuserimages-a.akamaihd.net/ugc/2/shot.jpg".into(),
            thumb: false,
            link: Some("https://x/full.jpg".into()),
        }));
        assert!(b.iter().any(|x| matches!(x, Block::Quote(q) if matches!(&q[..], [Block::Paragraph(s)] if plain(s) == "Quoted tip"))));
        assert_eq!(b.last(), Some(&Block::Paragraph(vec![Span { text: "Go left.".into(), bold: false, italic: false, link: None }])));
    }

    #[test]
    fn markdown_guides() {
        let g = from_markdown("Notes", "# Boss\nUse fire.\nReally.\n\n- item\n> tip\n![](https://x/y.png)");
        assert_eq!(g.blocks[0], Block::Heading(1, "Boss".into()));
        assert!(matches!(&g.blocks[1], Block::Paragraph(s) if plain(s) == "Use fire. Really."));
        assert!(matches!(&g.blocks[2], Block::Item(_)));
        assert!(matches!(&g.blocks[3], Block::Quote(_)));
        assert_eq!(g.blocks[4], Block::Image { url: "https://x/y.png".into(), thumb: false, link: None });
    }

    #[test]
    fn link_unwrapping() {
        assert_eq!(unwrap_link("https://steamcommunity.com/linkfilter/?url=https%3A%2F%2Fa.b%2Fc%3Fd%3De"), "https://a.b/c?d=e");
        assert_eq!(unwrap_link("https://steamcommunity.com/linkfilter/?u=https%3A%2F%2Fmapgenie.io%2Fx"), "https://mapgenie.io/x");
        assert_eq!(unwrap_link("https://a.b"), "https://a.b");
    }
}


/// Tests against trimmed copies of real Steam Community pages.
#[cfg(test)]
mod real_pages {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/steam/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn guide_list_page() {
        let p = parse_page(&fixture("guides-list-620.html"));
        assert_eq!(p.total, Some(1662));
        assert!(p.has_more);
        assert!(!p.content_check);
        assert_eq!(p.items.len(), 3);
        let g = &p.items[0];
        assert_eq!(g.id, "2936034273");
        assert_eq!(g.title, "100% Achievement Guide: Portal 2");
        assert_eq!(g.author, "Cynic 0055");
        assert_eq!(g.stars, Some(5));
        assert!(g.description.starts_with("This is a comprehensive 100% Achievement guide"));
        // The real preview, not the blurred low-res background behind it.
        assert!(g.preview.as_deref().unwrap().contains("imw=200"));
        assert_eq!(p.items[1].author, "Marik78");
    }

    #[test]
    fn mature_content_check() {
        let p = parse_page(&fixture("guides-content-check.html"));
        assert!(p.items.is_empty());
        assert!(p.content_check);
    }

    #[test]
    fn paging_without_info() {
        let p = parse_page(r#"<a class="pagebtn" href="?p=2">&gt;</a>"#);
        assert!(p.has_more && p.total.is_none());
        assert!(!parse_page("<div class=workshopBrowsePagingInfo>Showing 1-12 of 12 entries</div>").has_more);
    }

    #[test]
    fn table_guide() {
        let g = parse_guide(&fixture("guide-table.html"));
        assert_eq!(g.title, "100% Achievement Guide: Portal 2");
        assert_eq!(g.authors, "Cynic 0055 and 1 collaborators");
        assert_eq!(g.blocks[0], Block::Heading(1, "Introduction:".into()));
        assert!(matches!(&g.blocks[1], Block::Image { thumb: false, link: Some(_), .. }));
        let Block::Table(rows) = &g.blocks[2] else { panic!("{:?}", g.blocks[2]) };
        assert_eq!(rows.len(), 9);
        assert!(rows[0].iter().all(|c| c.header));
        assert_eq!(plain(&rows[0][0].spans), "Achievement Breakdown: (51)");
        assert!(rows[1..].iter().all(|r| r.iter().all(|c| !c.header)));
    }

    #[test]
    fn quotes_keep_their_content() {
        let g = parse_guide(&fixture("guide-quote.html"));
        assert_eq!(g.title, "Terraria: The Complete 100% Achievement Guide (1.4.5 UPDATED)");
        let quotes: Vec<&Vec<Block>> = g.blocks.iter().filter_map(|b| if let Block::Quote(q) = b { Some(q) } else { None }).collect();
        assert!(quotes.len() >= 3);
        assert!(quotes[0].iter().any(|b| matches!(b, Block::Heading(1, h) if h.starts_with("ATTENTION"))));
        assert!(quotes.iter().any(|q| matches!(q.first(), Some(Block::Image { .. }))));
    }

    #[test]
    fn thumbnails() {
        let g = parse_guide(&fixture("guide-thumbs.html"));
        assert_eq!(g.title, "Ultimate SDV Fishing Guide (1.6)");
        let thumbs = g.blocks.iter().filter(|b| matches!(b, Block::Image { thumb: true, .. })).count();
        assert!(thumbs > 10, "{thumbs}");
        let i = g.blocks.iter().position(|b| matches!(b, Block::Image { thumb: true, .. })).unwrap();
        let Block::Paragraph(p) = &g.blocks[i + 1] else { panic!() };
        assert_eq!(plain(p), "Anchovy\nArea: Ocean | Time: Any | Weather: Any");
    }

    #[test]
    fn external_links() {
        let g = parse_guide(&fixture("guide-links.html"));
        let links: Vec<&Span> = g.blocks.iter().flat_map(|b| if let Block::Paragraph(s) = b { s.iter() } else { [].iter() }).filter(|s| s.link.is_some()).collect();
        assert_eq!(links[0].text, "The Lands Between");
        assert_eq!(links[0].link.as_deref(), Some("https://mapgenie.io/elden-ring/maps/the-lands-between"));
        // The " [mapgenie.io]" host hint Steam appends is not part of the text.
        assert!(g.blocks.iter().all(|b| !format!("{b:?}").contains("[mapgenie.io]")));
    }

    #[test]
    fn languages() {
        assert_eq!(steam_language("en_US.UTF-8"), Some("english"));
        assert_eq!(steam_language("pt_BR.UTF-8"), Some("brazilian"));
        assert_eq!(steam_language("zh_TW"), Some("tchinese"));
        assert_eq!(steam_language("es_MX.UTF-8"), Some("latam"));
        assert_eq!(list_url("620", 2, Some("english")), "https://steamcommunity.com/app/620/guides/?browsefilter=toprated&p=2&requiredtags%5B%5D=english");
    }
}
