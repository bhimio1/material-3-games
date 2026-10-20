//! The per-game "extras" shared by the detail page, big picture and the
//! Game Mode overlay: achievements, guides (with a native reader), news,
//! notes and screenshots.

use crate::config::{Config, PinnedGuide};
use crate::library::Game;
use crate::steam::achievements::Achievements;
use crate::steam::guides::{self, Block, Guide, GuideItem, Span};
use crate::steam::news::NewsItem;
use crate::sys::task::Task;
use crate::thumbs::Thumbs;
use crate::ui::widgets::{self, ButtonKind};
use crate::ui::common;
use egui::{vec2, CornerRadius, RichText, Ui};
use egui_material_icons::icons::*;
use std::collections::HashMap;
use std::path::PathBuf;

/// Remote images (achievement icons, guide pictures) downloaded into the
/// cache on worker threads.
#[derive(Default)]
pub struct RemoteImages {
    tasks: HashMap<String, Task<Result<PathBuf, String>>>,
}

impl RemoteImages {
    const MAX_IN_FLIGHT: usize = 6;

    pub fn path(&mut self, ctx: &egui::Context, url: &str) -> Option<PathBuf> {
        if !self.tasks.contains_key(url) {
            let busy = self.tasks.values().filter(|t| t.running()).count();
            if busy >= Self::MAX_IN_FLIGHT {
                return None;
            }
            let mut t = Task::default();
            let u = url.to_string();
            t.spawn(ctx, move || crate::steam::web::fetch_file("images", &u));
            self.tasks.insert(url.to_string(), t);
        }
        let t = self.tasks.get_mut(url)?;
        t.poll();
        t.value.as_ref().and_then(|r| r.as_ref().ok()).cloned()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Achievements,
    Guides,
    News,
    Notes,
    Screenshots,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Tab::Achievements => "Achievements",
            Tab::Guides => "Guides",
            Tab::News => "News",
            Tab::Notes => "Notes",
            Tab::Screenshots => "Screenshots",
        }
    }

    pub fn icon(self) -> egui_material_icons::MaterialIcon {
        match self {
            Tab::Achievements => ICON_TROPHY,
            Tab::Guides => ICON_MENU_BOOK,
            Tab::News => ICON_NEWSPAPER,
            Tab::Notes => ICON_EDIT_NOTE,
            Tab::Screenshots => ICON_PHOTO_LIBRARY,
        }
    }

    pub fn for_game(game: &Game) -> Vec<Tab> {
        if game.steam_appid().is_some() {
            vec![Tab::Achievements, Tab::Guides, Tab::News, Tab::Notes, Tab::Screenshots]
        } else {
            vec![Tab::Guides, Tab::Notes, Tab::Screenshots]
        }
    }
}

pub enum Out {
    SaveConfig,
    OpenUrl(String),
    OpenPath(PathBuf),
}

pub struct Extras {
    pub game_id: String,
    pub tab: Tab,
    achievements: Task<Option<Achievements>>,
    guide_list: Task<Result<Vec<GuideItem>, String>>,
    news: Task<Result<Vec<NewsItem>, String>>,
    guide: Task<Result<Guide, String>>,
    /// The guide being read (as it would be pinned).
    pub reading: Option<PinnedGuide>,
    shots: Task<Vec<PathBuf>>,
    pub images: RemoteImages,
    add_guide: String,
    notes: Option<String>,
    show_hidden: bool,
}

impl Extras {
    pub fn new(game: &Game) -> Self {
        // Development: open a tab directly for screenshots.
        let dev_tab = std::env::var("M3_GAMES_TAB").ok().and_then(|t| Tab::for_game(game).into_iter().find(|x| x.label().eq_ignore_ascii_case(&t)));
        Self {
            game_id: game.id.clone(),
            tab: dev_tab.unwrap_or(Tab::for_game(game)[0]),
            achievements: Task::default(),
            guide_list: Task::default(),
            news: Task::default(),
            guide: Task::default(),
            reading: std::env::var("M3_GAMES_GUIDE").ok().map(|id| PinnedGuide { title: "Guide".into(), steam_id: id, location: String::new() }),
            shots: Task::default(),
            images: RemoteImages::default(),
            add_guide: String::new(),
            notes: None,
            show_hidden: false,
        }
    }

    /// Reuse `slot` for `game`, or start fresh when it's another game.
    pub fn for_game<'a>(slot: &'a mut Option<Extras>, game: &Game) -> &'a mut Extras {
        if slot.as_ref().is_none_or(|e| e.game_id != game.id) {
            *slot = Some(Extras::new(game));
        }
        slot.as_mut().expect("just set")
    }

    pub fn open_guide(&mut self, ctx: &egui::Context, g: PinnedGuide) {
        let pinned = g.clone();
        self.guide = Task::default();
        self.guide.spawn(ctx, move || load_guide(&pinned));
        self.reading = Some(g);
    }

    fn achievements_loaded(&mut self, ctx: &egui::Context, appid: &str, config: &Config) {
        if self.achievements.value.is_none() && !self.achievements.running() {
            let appid = appid.to_string();
            let key = config.steam_api_key.clone();
            let steamid = if key.is_empty() { String::new() } else { config.steamid64() };
            self.achievements.spawn(ctx, move || {
                let home = crate::sys::paths::home();
                let root = crate::steam::account::root(&home);
                let account = root.as_deref().and_then(crate::steam::account::user_account_id);
                crate::steam::achievements::load(root.as_deref(), account, &appid, &key, &steamid)
            });
        }
        self.achievements.poll();
    }
}

/// Read a pinned guide: a Steam guide, a local Markdown/text file, or a
/// Steam guide URL.
pub fn load_guide(g: &PinnedGuide) -> Result<Guide, String> {
    if !g.steam_id.is_empty() {
        return guides::fetch_guide(&g.steam_id);
    }
    let loc = g.location.trim();
    if let Some(id) = loc.split("id=").nth(1).map(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>()).filter(|s| !s.is_empty()) {
        if loc.contains("steamcommunity.com") {
            return guides::fetch_guide(&id);
        }
    }
    let path = crate::sys::paths::expand(loc);
    if path.is_file() {
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let title = if g.title.is_empty() { path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { g.title.clone() };
        return Ok(guides::from_markdown(&title, &text));
    }
    Err("Only Steam guides and local Markdown/text files open here. Web pages open in your browser.".into())
}

/// The tab strip.
pub fn tabs(ui: &mut Ui, extras: &mut Extras, game: &Game) -> bool {
    let options: Vec<(Tab, egui_material_icons::MaterialIcon, &str)> = Tab::for_game(game).into_iter().map(|t| (t, t.icon(), t.label())).collect();
    if !options.iter().any(|(t, _, _)| *t == extras.tab) {
        extras.tab = options[0].0;
    }
    widgets::segmented(ui, &mut extras.tab, &options)
}

/// The current tab's content. `compact` lays things out for the overlay's
/// narrow side sheet.
pub fn show(ui: &mut Ui, extras: &mut Extras, game: &Game, config: &mut Config, thumbs: &mut Thumbs, compact: bool) -> Vec<Out> {
    let ctx = ui.ctx().clone();
    let mut out = Vec::new();
    thumbs.poll();
    if extras.reading.is_some() {
        reader(ui, extras, game, config, thumbs, &mut out);
        return out;
    }
    match extras.tab {
        Tab::Achievements => {
            if let Some(appid) = game.steam_appid() {
                extras.achievements_loaded(&ctx, appid, config);
                achievements(ui, extras, thumbs, compact);
            }
        }
        Tab::Guides => guide_tab(ui, extras, game, config, thumbs, compact, &mut out),
        Tab::News => {
            if let Some(appid) = game.steam_appid() {
                if extras.news.value.is_none() && !extras.news.running() {
                    let a = appid.to_string();
                    extras.news.spawn(&ctx, move || crate::steam::news::fetch(&a));
                }
                extras.news.poll();
                news(ui, extras, &mut out);
            }
        }
        Tab::Notes => notes(ui, extras, game, config, &mut out),
        Tab::Screenshots => screenshots(ui, extras, game, thumbs, compact, &mut out),
    }
    out
}

fn achievements(ui: &mut Ui, extras: &mut Extras, thumbs: &mut Thumbs, compact: bool) {
    let p = widgets::palette(ui);
    let ctx = ui.ctx().clone();
    let Some(result) = extras.achievements.value.as_ref() else {
        common::loading(ui, "Reading achievements…");
        return;
    };
    let Some(a) = result.clone() else {
        common::empty_hint(ui, "No achievements found. Play the game once with Steam, or add a Web API key in Settings.");
        return;
    };
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), egui::Sense::hover());
        progress_ring(ui, rect.center(), 24.0, a.progress(), p.primary, p.surface_container_highest);
        ui.vertical(|ui| {
            ui.label(widgets::title_text(format!("{} of {} unlocked", a.unlocked(), a.total()), 16.0).color(p.on_surface));
            ui.label(RichText::new(format!("{:.0}% complete", a.progress() * 100.0)).color(p.on_surface_variant));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut extras.show_hidden, "Show hidden");
        });
    });
    ui.add_space(6.0);
    let icon = if compact { 40.0 } else { 48.0 };
    for x in &a.list {
        let secret = x.hidden && !x.unlocked && !extras.show_hidden;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(icon, icon), egui::Sense::hover());
            let url = if x.unlocked { x.icon_url.as_deref() } else { x.icon_gray_url.as_deref().or(x.icon_url.as_deref()) };
            let path = url.and_then(|u| extras.images.path(&ctx, u));
            crate::ui::art::paint(ui, thumbs, path.as_deref(), rect, 8, if x.unlocked { ICON_TROPHY } else { ICON_LOCK });
            ui.vertical(|ui| {
                let title = if secret { "Hidden achievement".to_string() } else { x.name.clone() };
                ui.label(widgets::title_text(title, 14.5).color(if x.unlocked { p.on_surface } else { p.on_surface_variant }));
                let mut sub = if secret { "Keep playing to find out.".to_string() } else { x.description.clone() };
                if let Some(t) = x.unlock_time {
                    let when = chrono::DateTime::from_timestamp(t, 0).map(|d| d.with_timezone(&chrono::Local).format("%b %e, %Y").to_string()).unwrap_or_default();
                    sub = if sub.is_empty() { format!("Unlocked {when}") } else { format!("{sub} · Unlocked {when}") };
                }
                if let Some(g) = x.global_percent {
                    sub.push_str(&format!(" · {g:.1}% of players"));
                }
                ui.add(egui::Label::new(RichText::new(sub).size(12.5).color(p.on_surface_variant)).wrap());
            });
        });
        ui.add_space(4.0);
    }
}

pub fn progress_ring(ui: &Ui, center: egui::Pos2, radius: f32, t: f32, fg: egui::Color32, bg: egui::Color32) {
    let painter = ui.painter();
    painter.circle_stroke(center, radius, egui::Stroke::new(6.0, bg));
    let n = 64;
    let end = (t.clamp(0.0, 1.0) * n as f32) as usize;
    if end > 0 {
        let pts: Vec<egui::Pos2> = (0..=end)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 / n as f32 * std::f32::consts::TAU;
                center + vec2(a.cos(), a.sin()) * radius
            })
            .collect();
        painter.add(egui::Shape::line(pts, egui::Stroke::new(6.0, fg)));
    }
}

fn guide_tab(ui: &mut Ui, extras: &mut Extras, game: &Game, config: &mut Config, thumbs: &mut Thumbs, compact: bool, out: &mut Vec<Out>) {
    let p = widgets::palette(ui);
    let ctx = ui.ctx().clone();
    let pinned = config.options(&game.id).guides;
    if !pinned.is_empty() {
        ui.add_space(6.0);
        ui.label(widgets::title_text("Pinned", 14.0).color(p.primary));
        for g in pinned {
            let title = if g.title.is_empty() { g.location.clone() } else { g.title.clone() };
            if common::list_row(ui, ICON_PUSH_PIN, true, &title, if g.steam_id.is_empty() { &g.location } else { "Steam guide" }, |_| {}).clicked() {
                if g.steam_id.is_empty() && g.location.starts_with("http") && !g.location.contains("steamcommunity.com") {
                    out.push(Out::OpenUrl(g.location.clone()));
                } else {
                    extras.open_guide(&ctx, g);
                }
            }
        }
    }
    // Pin any web page or local Markdown/text file.
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let w = ui.available_width() - 110.0;
        ui.add(egui::TextEdit::singleline(&mut extras.add_guide).hint_text("Pin a guide: URL or ~/path/notes.md").desired_width(w.max(120.0)));
        let ok = !extras.add_guide.trim().is_empty();
        if widgets::button_enabled(ui, ButtonKind::Tonal, Some(ICON_PUSH_PIN), "Pin", ok).clicked() {
            let loc = extras.add_guide.trim().to_string();
            let title = loc.rsplit('/').next().unwrap_or(&loc).to_string();
            let mut o = config.options(&game.id);
            o.guides.push(PinnedGuide { title, steam_id: String::new(), location: loc });
            config.games.insert(game.id.clone(), o);
            extras.add_guide.clear();
            out.push(Out::SaveConfig);
        }
    });
    let Some(appid) = game.steam_appid() else {
        return;
    };
    if extras.guide_list.value.is_none() && !extras.guide_list.running() {
        let a = appid.to_string();
        extras.guide_list.spawn(&ctx, move || guides::fetch_list(&a));
    }
    extras.guide_list.poll();
    ui.add_space(10.0);
    ui.label(widgets::title_text("Steam Community guides", 14.0).color(p.primary));
    match extras.guide_list.value.clone() {
        None => common::loading(ui, "Loading guides…"),
        Some(Err(e)) => common::empty_hint(ui, &e),
        Some(Ok(list)) => {
            for item in list {
                let stars = item.stars.map(|s| format!(" · {}", "★".repeat(s as usize))).unwrap_or_default();
                let sub = format!("{}{}", if item.author.is_empty() { "Guide".into() } else { format!("by {}", item.author) }, stars);
                let preview = if compact { None } else { item.preview.as_deref().and_then(|u| extras.images.path(&ctx, u)) };
                let resp = guide_row(ui, thumbs, preview.as_deref(), compact, &item.title, &sub);
                if resp.clicked() {
                    extras.open_guide(&ctx, PinnedGuide { title: item.title.clone(), steam_id: item.id.clone(), location: item.url() });
                }
            }
        }
    }
}

/// A guide in the list: preview image (or a book icon), title and byline.
fn guide_row(ui: &mut Ui, thumbs: &mut Thumbs, preview: Option<&std::path::Path>, compact: bool, title: &str, sub: &str) -> egui::Response {
    if compact {
        return common::list_row(ui, ICON_MENU_BOOK, false, title, sub, |_| {});
    }
    let p = widgets::palette(ui);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 76.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect.expand2(vec2(6.0, 0.0)), CornerRadius::same(14), p.on_surface.gamma_multiply(0.06));
    }
    let img = egui::Rect::from_min_size(rect.min + vec2(0.0, 6.0), vec2(64.0, 64.0));
    crate::ui::art::paint(ui, thumbs, preview, img, 12, ICON_MENU_BOOK);
    let left = img.right() + 14.0;
    let max_w = rect.right() - left - 8.0;
    let t = widgets::truncated_galley(ui, title, widgets::title_font(15.0), p.on_surface, max_w);
    let s = widgets::truncated_galley(ui, sub, egui::FontId::proportional(12.5), p.on_surface_variant, max_w);
    ui.painter().galley(egui::pos2(left, rect.center().y - t.size().y - 1.0), t, p.on_surface);
    ui.painter().galley(egui::pos2(left, rect.center().y + 2.0), s, p.on_surface_variant);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn reader(ui: &mut Ui, extras: &mut Extras, game: &Game, config: &mut Config, thumbs: &mut Thumbs, out: &mut Vec<Out>) {
    let p = widgets::palette(ui);
    let ctx = ui.ctx().clone();
    let current = extras.reading.clone().expect("reader is only shown while reading");
    if extras.guide.value.is_none() && !extras.guide.running() {
        let g = current.clone();
        extras.guide.spawn(&ctx, move || load_guide(&g));
    }
    extras.guide.poll();
    let mut opts = config.options(&game.id);
    let pinned = opts.guides.iter().any(|g| g == &current || (!current.steam_id.is_empty() && g.steam_id == current.steam_id));
    ui.horizontal(|ui| {
        if widgets::icon_button(ui, ICON_ARROW_BACK, "Back to guides", true).clicked() {
            extras.reading = None;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !current.location.is_empty() && current.location.starts_with("http") && widgets::icon_button(ui, ICON_OPEN_IN_NEW, "Open in browser", true).clicked() {
                out.push(Out::OpenUrl(current.location.clone()));
            }
            let (icon, tip) = if pinned { (ICON_KEEP_OFF, "Unpin") } else { (ICON_PUSH_PIN, "Pin to this game") };
            if widgets::icon_button(ui, icon, tip, true).clicked() {
                if pinned {
                    opts.guides.retain(|g| !(g == &current || (!current.steam_id.is_empty() && g.steam_id == current.steam_id)));
                } else {
                    opts.guides.push(current.clone());
                }
                config.games.insert(game.id.clone(), opts.clone());
                out.push(Out::SaveConfig);
            }
            let title = match &extras.guide.value {
                Some(Ok(g)) if !g.title.is_empty() => g.title.clone(),
                _ => current.title.clone(),
            };
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(widgets::title_text(title, 18.0).color(p.on_surface)).truncate());
            });
        });
    });
    if extras.reading.is_none() {
        return;
    }
    match extras.guide.value.clone() {
        None => common::loading(ui, "Opening guide…"),
        Some(Err(e)) => common::empty_hint(ui, &e),
        Some(Ok(g)) => {
            if !g.description.is_empty() {
                ui.add(egui::Label::new(RichText::new(&g.description).italics().color(p.on_surface_variant)).wrap());
            }
            ui.add_space(8.0);
            for block in &g.blocks {
                render_block(ui, block, &mut extras.images, thumbs, &ctx, out);
            }
        }
    }
}

fn spans_ui(ui: &mut Ui, spans: &[Span], out: &mut Vec<Out>) {
    let p = widgets::palette(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for s in spans {
            let mut t = RichText::new(&s.text).size(14.0).color(p.on_surface);
            if s.bold {
                t = t.family(crate::theme::style::medium_family());
            }
            if s.italic {
                t = t.italics();
            }
            match &s.link {
                Some(url) => {
                    if ui.add(egui::Link::new(t.color(p.primary))).on_hover_text(url).clicked() {
                        out.push(Out::OpenUrl(url.clone()));
                    }
                }
                None => {
                    ui.label(t);
                }
            }
        }
    });
}

fn render_block(ui: &mut Ui, block: &Block, images: &mut RemoteImages, thumbs: &mut Thumbs, ctx: &egui::Context, out: &mut Vec<Out>) {
    let p = widgets::palette(ui);
    match block {
        Block::Heading(level, text) => {
            ui.add_space(if *level == 1 { 14.0 } else { 8.0 });
            let size = match level {
                1 => 20.0,
                2 => 17.0,
                _ => 15.0,
            };
            ui.label(widgets::title_text(text, size).color(if *level == 1 { p.primary } else { p.on_surface }));
            ui.add_space(4.0);
        }
        Block::Paragraph(spans) => {
            spans_ui(ui, spans, out);
            ui.add_space(6.0);
        }
        Block::Item(spans) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("•").color(p.primary));
                spans_ui(ui, spans, out);
            });
        }
        Block::Quote(spans) => {
            egui::Frame::new().fill(p.surface_container_high).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                spans_ui(ui, spans, out);
            });
            ui.add_space(6.0);
        }
        Block::Image(url) => {
            let w = ui.available_width().min(640.0);
            match images.path(ctx, url).and_then(|path| thumbs.get(&path)) {
                Some(tex) => {
                    let [tw, th] = tex.size();
                    let h = w * th as f32 / tw.max(1) as f32;
                    ui.add(egui::Image::new(&tex).fit_to_exact_size(vec2(w, h)).corner_radius(12));
                }
                None => {
                    let (rect, _) = ui.allocate_exact_size(vec2(w, w * 0.5), egui::Sense::hover());
                    ui.painter().rect_filled(rect, CornerRadius::same(12), p.surface_container_high);
                    widgets::paint_icon(ui, ICON_IMAGE, rect.center(), 32.0, p.on_surface_variant);
                }
            }
            ui.add_space(6.0);
        }
    }
}

fn news(ui: &mut Ui, extras: &mut Extras, out: &mut Vec<Out>) {
    let p = widgets::palette(ui);
    match extras.news.value.clone() {
        None => common::loading(ui, "Loading news…"),
        Some(Err(e)) => common::empty_hint(ui, &format!("Couldn't load news: {e}")),
        Some(Ok(items)) if items.is_empty() => common::empty_hint(ui, "No news for this game."),
        Some(Ok(items)) => {
            for n in items {
                common::card(ui, "", |ui| {
                    ui.add_space(6.0);
                    ui.label(widgets::title_text(&n.title, 16.0).color(p.on_surface));
                    let when = chrono::DateTime::from_timestamp(n.date, 0).map(|d| d.with_timezone(&chrono::Local).format("%b %e, %Y").to_string()).unwrap_or_default();
                    ui.label(RichText::new(format!("{when} · {}", n.feedlabel)).size(12.5).color(p.on_surface_variant));
                    ui.add_space(6.0);
                    let preview: String = n.contents.chars().take(900).collect();
                    let more = n.contents.chars().count() > 900;
                    ui.add(egui::Label::new(RichText::new(if more { format!("{preview}…") } else { preview }).color(p.on_surface)).wrap());
                    if !n.url.is_empty() && widgets::button(ui, ButtonKind::Text, Some(ICON_OPEN_IN_NEW), "Read more").clicked() {
                        out.push(Out::OpenUrl(n.url.clone()));
                    }
                    ui.add_space(4.0);
                });
            }
        }
    }
}

fn notes(ui: &mut Ui, extras: &mut Extras, game: &Game, config: &mut Config, out: &mut Vec<Out>) {
    let p = widgets::palette(ui);
    let text = extras.notes.get_or_insert_with(|| config.options(&game.id).notes);
    ui.add_space(8.0);
    ui.label(RichText::new("Your notes for this game. Also in the Game Mode overlay.").color(p.on_surface_variant));
    ui.add_space(6.0);
    let r = ui.add(egui::TextEdit::multiline(text).desired_rows(12).desired_width(f32::INFINITY).hint_text("Codes, routes, boss tips…"));
    if r.lost_focus() || (r.changed() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
        let mut o = config.options(&game.id);
        if o.notes != *text {
            o.notes = text.clone();
            config.games.insert(game.id.clone(), o);
            out.push(Out::SaveConfig);
        }
    }
}

/// Where Game Mode screenshots of a game are saved.
pub fn screenshot_dir(game_name: &str) -> PathBuf {
    dirs::picture_dir()
        .unwrap_or_else(|| crate::sys::paths::home().join("Pictures"))
        .join("Screenshots/Material 3 Games")
        .join(crate::library::slugify(game_name))
}

fn list_images(dir: &std::path::Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "png" | "jpg" | "jpeg")))
                .collect()
        })
        .unwrap_or_default()
}

/// Steam's screenshots for the game (or its shortcut) plus Game Mode ones.
pub fn all_screenshots(game: &Game) -> Vec<PathBuf> {
    let mut shots = list_images(&screenshot_dir(&game.name));
    if let Some(user) = crate::steam::account::root(&crate::sys::paths::home()).and_then(|r| crate::steam::account::user_dir(&r)) {
        if let Some(appid) = game.steam_appid() {
            shots.extend(crate::steam::appconfig::screenshots(&user, appid));
        }
        if let Some(sc) = game.shortcut_appid {
            shots.extend(crate::steam::appconfig::screenshots(&user, &crate::steam::shortcuts::rungameid(sc).to_string()));
        }
    }
    shots.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).and_then(|m| m.modified()).ok()));
    shots
}

fn screenshots(ui: &mut Ui, extras: &mut Extras, game: &Game, thumbs: &mut Thumbs, compact: bool, out: &mut Vec<Out>) {
    let ctx = ui.ctx().clone();
    if extras.shots.value.is_none() && !extras.shots.running() {
        let g = game.clone();
        extras.shots.spawn(&ctx, move || all_screenshots(&g));
    }
    extras.shots.poll();
    let Some(shots) = extras.shots.value.clone() else {
        common::loading(ui, "Looking for screenshots…");
        return;
    };
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if widgets::button(ui, ButtonKind::Text, Some(ICON_FOLDER_OPEN), "Open folder").clicked() {
            let dir = screenshot_dir(&game.name);
            let _ = std::fs::create_dir_all(&dir);
            out.push(Out::OpenPath(dir));
        }
        if widgets::icon_button(ui, ICON_REFRESH, "Refresh", true).clicked() {
            extras.shots = Task::default();
        }
    });
    if shots.is_empty() {
        common::empty_hint(ui, "No screenshots yet. In Game Mode, press Guide + A.");
        return;
    }
    let cols = if compact { 2 } else { 3 };
    let gap = 8.0;
    let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).max(80.0);
    for chunk in shots.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for s in chunk {
                let (rect, resp) = ui.allocate_exact_size(vec2(w, w * 9.0 / 16.0), egui::Sense::click());
                crate::ui::art::paint(ui, thumbs, Some(s), rect, 10, ICON_IMAGE);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    out.push(Out::OpenPath(s.clone()));
                }
            }
        });
        ui.add_space(gap);
    }
}
