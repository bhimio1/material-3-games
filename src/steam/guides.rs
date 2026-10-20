//! Steam Community guides: the list for a game and a native reader for a
//! guide page. Steam has no public API for guide content, so both are read
//! from the community HTML pages. The parsers look for the class names
//! Steam has used for years and fall back gracefully when they change.

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

pub fn list_url(appid: &str) -> String {
    format!("https://steamcommunity.com/app/{appid}/guides/?browsefilter=toprated&p=1")
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

/// Parse a game's guide list page.
pub fn parse_list(html: &str) -> Vec<GuideItem> {
    let doc = Html::parse_document(html);
    let mut out: Vec<GuideItem> = Vec::new();
    let links = sel("a[href*='filedetails/?id=']");
    let title_sel = sel(".workshopItemTitle");
    let desc_sel = sel(".workshopItemShortDesc");
    let author_sel = sel(".workshopItemAuthorName");
    let img_sel = sel("img.workshopItemPreviewImage, img");
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
            preview: container.select(&img_sel).next().and_then(|i| i.value().attr("src")).map(str::to_string),
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
    Image(String),
    Quote(Vec<Span>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guide {
    pub title: String,
    pub description: String,
    pub authors: String,
    pub blocks: Vec<Block>,
}

/// Steam wraps external links in its link filter.
fn unwrap_link(href: &str) -> String {
    if let Some(i) = href.find("linkfilter/?url=") {
        let rest = &href[i + "linkfilter/?url=".len()..];
        return percent_decode(rest);
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

    fn take_spans(&mut self) -> Vec<Span> {
        let before = self.blocks.len();
        self.flush();
        if self.blocks.len() > before {
            if let Some(Block::Paragraph(s)) = self.blocks.pop() {
                return s;
            }
        }
        Vec::new()
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
                            b.flush();
                            b.blocks.push(Block::Image(src.to_string()));
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
                        walk(child, &mut inner, Style { italic: true, ..style }, link);
                        let spans = inner.take_spans();
                        if !spans.is_empty() {
                            b.blocks.push(Block::Quote(spans));
                        }
                    }
                    "ul" | "ol" | "p" | "div" | "table" | "tr" => {
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
        title: first_text(".guideTopTitle"),
        description: first_text(".guideTopDescription"),
        authors: first_text(".guideAuthors, .friendBlockContent"),
        blocks: Vec::new(),
    };
    if g.title.is_empty() {
        g.title = first_text(".workshopItemTitle, title");
    }
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

/// Plain-text version of a span list.
#[cfg(test)]
pub fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

/// Guide list for an app (cached for a day, offline from cache).
pub fn fetch_list(appid: &str) -> Result<Vec<GuideItem>, String> {
    let html = super::web::get_cached("guides", &format!("list-{appid}"), &list_url(appid), Duration::from_secs(24 * 3600))?;
    let list = parse_list(&html);
    if list.is_empty() {
        return Err("No guides found (or Steam changed its page layout).".into());
    }
    Ok(list)
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
            blocks.push(Block::Quote(span(q)));
        } else if l.starts_with("![") {
            flush(&mut para, &mut blocks);
            if let Some(url) = l.split_once("](").map(|(_, r)| r.trim_end_matches(')')) {
                blocks.push(Block::Image(url.to_string()));
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
        assert!(b.contains(&Block::Image("https://steamuserimages-a.akamaihd.net/ugc/2/shot.jpg".into())));
        assert!(b.iter().any(|x| matches!(x, Block::Quote(s) if plain(s) == "Quoted tip")));
        assert_eq!(b.last(), Some(&Block::Paragraph(vec![Span { text: "Go left.".into(), bold: false, italic: false, link: None }])));
    }

    #[test]
    fn markdown_guides() {
        let g = from_markdown("Notes", "# Boss\nUse fire.\nReally.\n\n- item\n> tip\n![](https://x/y.png)");
        assert_eq!(g.blocks[0], Block::Heading(1, "Boss".into()));
        assert!(matches!(&g.blocks[1], Block::Paragraph(s) if plain(s) == "Use fire. Really."));
        assert!(matches!(&g.blocks[2], Block::Item(_)));
        assert!(matches!(&g.blocks[3], Block::Quote(_)));
        assert_eq!(g.blocks[4], Block::Image("https://x/y.png".into()));
    }

    #[test]
    fn link_unwrapping() {
        assert_eq!(unwrap_link("https://steamcommunity.com/linkfilter/?url=https%3A%2F%2Fa.b%2Fc%3Fd%3De"), "https://a.b/c?d=e");
        assert_eq!(unwrap_link("https://a.b"), "https://a.b");
    }
}
