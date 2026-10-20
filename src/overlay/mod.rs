//! The Game Mode quick overlay (`m3-games __overlay`): a full-screen X11
//! window in gamescope marked `STEAM_OVERLAY`, invisible until the Guide
//! button (or `m3-games overlay toggle`) shows it over the running game.
//! It carries performance controls (FPS limit, tearing, upscaling,
//! MangoHud), audio, brightness, media, controller batteries, the current
//! game's achievements, guides, notes and screenshots, and quit/exit/power.

pub mod ipc;
pub mod ui;

use crate::config::{Config, OverlayPrefs};
use crate::gamepad::{GuideAction, Input, Nav};
use crate::gamescope::{self, Ctl};
use crate::library::Game;
use crate::session::{self, Running};
use crate::sys::quick;
use crate::sys::task::Task;
use crate::theme::{sources, style, M3Palette};
use crate::thumbs::Thumbs;
use ipc::Msg;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn shown_flag() -> PathBuf {
    session::runtime_dir().join("overlay-shown")
}

/// The overlay is currently on screen (for big picture's input handling).
/// A flag left behind by an overlay that died doesn't count.
pub fn visible() -> bool {
    shown_flag().exists() && !session::find_processes(&["__overlay"]).is_empty()
}

/// Apply a game's gamescope settings.
pub fn apply_prefs(ctl: &Ctl, prefs: &OverlayPrefs) {
    let _ = ctl.fps_limit(prefs.fps_limit);
    let _ = ctl.allow_tearing(prefs.allow_tearing);
    let _ = ctl.scaling(prefs.filter, prefs.sharpness);
    if mangohud_available() {
        let _ = gamescope::set_perf_overlay(prefs.perf_overlay);
    }
}

/// gamescope runs mangoapp when started with `--mangoapp`; m3-session and
/// nested Game Mode export MANGOHUD_CONFIGFILE in that case.
pub fn mangohud_available() -> bool {
    std::env::var_os("MANGOHUD_CONFIGFILE").is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    QuitGame,
    ExitDesktop,
    Power(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Quick,
    Extras(crate::ui::steamui::Tab),
}

pub struct Overlay {
    pub ctx: egui::Context,
    pub palette: M3Palette,
    pub config: Config,
    pub games: Vec<Game>,
    scan: Task<Vec<Game>>,
    pub thumbs: Thumbs,
    pub shown: bool,
    window: Option<u32>,
    pub ctl: Option<Ctl>,
    ipc: Option<flume::Receiver<Msg>>,
    pad: Option<flume::Receiver<Input>>,
    pub running: Option<Running>,
    pub section: Section,
    pub extras: Option<crate::ui::steamui::Extras>,
    pub audio: Task<Option<quick::Audio>>,
    pub brightness: Task<Option<u32>>,
    pub media: Task<Option<quick::Media>>,
    pub batteries: Task<Vec<quick::Battery>>,
    refreshed: Option<Instant>,
    pub confirm: Option<Confirm>,
    pub shot: Task<Result<PathBuf, String>>,
    pub toast: Option<(String, Instant)>,
    pending_keys: Vec<egui::Event>,
}

impl Overlay {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        style::install_fonts(&ctx);
        let window = gamescope::x11_window(cc);
        let ctl = Ctl::connect().ok();
        if let (Some(c), Some(w)) = (&ctl, window) {
            let _ = c.set(w, "STEAM_GAME", &[gamescope::OVERLAY_APPID]);
            let _ = c.overlay(w, false);
        }
        let wake = ctx.clone();
        let ipc = ipc::serve(&ipc::socket_path(), move || wake.request_repaint()).ok();
        let pad = crate::gamepad::start_inputs(ctx.clone(), false);
        let mut o = Self {
            palette: M3Palette::from_seed(crate::theme::palette::DEFAULT_SEED, true),
            config: Config::load(),
            games: Vec::new(),
            scan: Task::default(),
            thumbs: Thumbs::new(ctx.clone()),
            shown: std::env::var_os("M3_GAMES_OVERLAY_SHOWN").is_some(),
            window,
            ctl,
            ipc,
            pad,
            running: session::read_running(),
            section: Section::Quick,
            extras: None,
            audio: Task::default(),
            brightness: Task::default(),
            media: Task::default(),
            batteries: Task::default(),
            refreshed: None,
            confirm: None,
            shot: Task::default(),
            toast: None,
            pending_keys: Vec::new(),
            ctx,
        };
        // Development: open a section directly for screenshots.
        if let Ok(name) = std::env::var("M3_GAMES_OVERLAY_SECTION") {
            use crate::ui::steamui::Tab;
            let tabs = [Tab::Achievements, Tab::Guides, Tab::News, Tab::Notes, Tab::Screenshots];
            if let Some(t) = tabs.into_iter().find(|t| t.label().eq_ignore_ascii_case(&name)) {
                o.section = Section::Extras(t);
            }
        }
        if !o.shown {
            let _ = std::fs::remove_file(shown_flag());
        }
        o.apply_theme();
        o.rescan();
        o
    }

    fn apply_theme(&mut self) {
        let palette = match self.config.theme_source {
            crate::config::ThemeSource::Auto => sources::auto_theme_path().and_then(|p| M3Palette::from_theme_json(&p).ok()),
            crate::config::ThemeSource::Seed => None,
        }
        .unwrap_or_else(|| M3Palette::from_seed(&self.config.seed_color, true));
        style::apply(&self.ctx, &palette);
        crate::ui::widgets::set_palette(&self.ctx, palette.clone());
        crate::ui::widgets::set_filled_icons(self.config.filled_icons);
        self.ctx.set_zoom_factor(self.config.ui_scale.clamp(0.6, 2.5) * 1.25);
        self.palette = palette;
    }

    fn rescan(&mut self) {
        let enabled = self.config.enabled_local();
        self.scan.spawn(&self.ctx, move || {
            let mut g = crate::library::scan(&crate::sys::paths::home(), enabled);
            crate::covers::apply_cached(&mut g);
            g
        });
    }

    pub fn game(&self) -> Option<&Game> {
        let id = &self.running.as_ref()?.id;
        self.games.iter().find(|g| &g.id == id)
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    pub fn set_shown(&mut self, shown: bool) {
        self.shown = shown;
        if shown {
            let _ = std::fs::create_dir_all(session::runtime_dir());
            let _ = std::fs::write(shown_flag(), b"1");
        } else {
            let _ = std::fs::remove_file(shown_flag());
        }
        if let (Some(c), Some(w)) = (&self.ctl, self.window) {
            let _ = c.overlay(w, shown);
        }
        if shown {
            self.running = session::read_running();
            self.config = Config::load();
            self.refresh(true);
        } else {
            self.confirm = None;
        }
        self.ctx.request_repaint();
    }

    /// Re-read volume, brightness, media and batteries (every few seconds
    /// while shown).
    fn refresh(&mut self, now: bool) {
        if !now && self.refreshed.is_some_and(|t| t.elapsed() < Duration::from_secs(4)) {
            return;
        }
        self.refreshed = Some(Instant::now());
        if !self.audio.running() {
            self.audio.spawn(&self.ctx, quick::audio);
        }
        if !self.brightness.running() {
            self.brightness.spawn(&self.ctx, quick::brightness);
        }
        if !self.media.running() {
            self.media.spawn(&self.ctx, quick::media);
        }
        if !self.batteries.running() {
            self.batteries.spawn(&self.ctx, || quick::device_batteries(std::path::Path::new("/sys/class/power_supply")));
        }
    }

    pub fn prefs(&self) -> OverlayPrefs {
        self.running.as_ref().map(|r| self.config.options(&r.id).overlay).unwrap_or_default()
    }

    pub fn set_prefs(&mut self, prefs: OverlayPrefs) {
        if let Some(c) = &self.ctl {
            apply_prefs(c, &prefs);
        }
        if let Some(r) = &self.running {
            let mut o = self.config.options(&r.id);
            o.overlay = prefs;
            self.config.games.insert(r.id.clone(), o);
            let _ = self.config.save();
        }
    }

    /// Take a screenshot through gamescope, then file it under the game.
    pub fn screenshot(&mut self) {
        if self.shot.running() {
            return;
        }
        let was_shown = self.shown;
        if was_shown {
            self.set_shown(false);
        }
        let name = self.game().map(|g| g.name.clone()).unwrap_or_else(|| "Game Mode".into());
        self.shot.spawn(&self.ctx, move || {
            // Let gamescope drop the overlay from the next frame first.
            std::thread::sleep(Duration::from_millis(250));
            take_screenshot(&name)
        });
    }

    pub fn quit_game(&mut self) {
        let Some(r) = self.running.clone() else {
            return;
        };
        let steam = self.game().and_then(|g| g.steam_appid().map(str::to_string));
        match crate::gamesession::stop(&r, steam.as_deref()) {
            Ok(()) => {
                self.notify(format!("Closing {}", r.name));
                self.set_shown(false);
            }
            Err(e) => self.notify(e),
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Toggle => self.set_shown(!self.shown),
            Msg::Show => self.set_shown(true),
            Msg::Hide => self.set_shown(false),
            Msg::Screenshot => self.screenshot(),
            Msg::CloseGame => {
                self.set_shown(true);
                self.confirm = Some(Confirm::QuitGame);
            }
            Msg::GameStarted => {
                self.running = session::read_running();
                self.config = Config::load();
                self.extras = None;
                self.rescan();
            }
            Msg::GameEnded => {
                self.running = None;
                self.extras = None;
                self.section = Section::Quick;
                if let Some(c) = &self.ctl {
                    apply_prefs(c, &OverlayPrefs::default());
                }
            }
        }
    }

    fn poll(&mut self) {
        self.thumbs.poll();
        if self.scan.poll() {
            if let Some(g) = self.scan.value.take() {
                self.games = g;
            }
        }
        let msgs: Vec<Msg> = self.ipc.as_ref().map(|r| r.try_iter().collect()).unwrap_or_default();
        for m in msgs {
            self.handle(m);
        }
        let inputs: Vec<Input> = self.pad.as_ref().map(|r| r.try_iter().collect()).unwrap_or_default();
        for i in inputs {
            match i {
                Input::Guide(GuideAction::Toggle) => self.set_shown(!self.shown),
                Input::Guide(GuideAction::Screenshot) => self.screenshot(),
                Input::Guide(GuideAction::CloseGame) => self.handle(Msg::CloseGame),
                Input::Nav(n) if self.shown => self.nav(n),
                Input::Nav(_) => {}
            }
        }
        if let Some(r) = self.shot.take() {
            match r {
                Ok(p) => self.notify(format!("Screenshot saved to {}", crate::sys::paths::tilde(&p))),
                Err(e) => self.notify(format!("Screenshot failed: {e}")),
            }
        }
        for t in [&mut self.audio as &mut dyn Pollable, &mut self.brightness, &mut self.media, &mut self.batteries] {
            t.poll_any();
        }
        if self.shown {
            self.refresh(false);
            self.ctx.request_repaint_after(Duration::from_secs(1));
        }
        if self.toast.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(4)) {
            self.toast = None;
        }
    }

    /// Controller navigation becomes keyboard focus movement.
    fn nav(&mut self, n: Nav) {
        let key = |k: egui::Key, shift: bool| egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE },
        };
        match n {
            Nav::Down => self.pending_keys.push(key(egui::Key::Tab, false)),
            Nav::Up => self.pending_keys.push(key(egui::Key::Tab, true)),
            Nav::Left => self.pending_keys.push(key(egui::Key::ArrowLeft, false)),
            Nav::Right => self.pending_keys.push(key(egui::Key::ArrowRight, false)),
            Nav::Accept => self.pending_keys.push(key(egui::Key::Enter, false)),
            Nav::Back => {
                if self.confirm.is_some() {
                    self.confirm = None;
                } else if self.extras.as_ref().is_some_and(|e| e.reading.is_some()) {
                    if let Some(e) = &mut self.extras {
                        e.reading = None;
                    }
                } else if self.section != Section::Quick {
                    self.section = Section::Quick;
                } else {
                    self.set_shown(false);
                }
            }
            Nav::Options | Nav::Menu => {}
        }
        self.ctx.request_repaint();
    }
}

trait Pollable {
    fn poll_any(&mut self);
}

impl<T: Send + 'static> Pollable for Task<T> {
    fn poll_any(&mut self) {
        self.poll();
    }
}

/// Ask gamescope for a screenshot and copy it into the game's folder.
fn take_screenshot(game_name: &str) -> Result<PathBuf, String> {
    let ctl = Ctl::connect()?;
    let src = PathBuf::from(gamescope::SCREENSHOT_PATH);
    let before = std::fs::metadata(&src).and_then(|m| m.modified()).ok();
    ctl.screenshot(gamescope::ScreenshotType::AllRealLayers)?;
    let start = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(150));
        let now = std::fs::metadata(&src).and_then(|m| m.modified()).ok();
        if now.is_some() && now != before {
            // gamescope writes in a thread; give it a moment to finish.
            std::thread::sleep(Duration::from_millis(300));
            break;
        }
        if start.elapsed() > Duration::from_secs(5) {
            return Err("gamescope didn't save a screenshot".into());
        }
    }
    let dir = crate::ui::steamui::screenshot_dir(game_name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.png", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")));
    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

impl eframe::App for Overlay {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        raw.events.append(&mut self.pending_keys);
    }

    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::show(ui, self);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

/// `m3-games __overlay`.
pub fn run() -> eframe::Result {
    let windowed = std::env::var_os("M3_GAMES_WINDOWED").is_some();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Quick settings")
        .with_app_id("material_3_games_overlay")
        .with_transparent(true)
        .with_decorations(false)
        .with_inner_size([1240.0, 820.0]);
    if !windowed {
        viewport = viewport.with_fullscreen(true);
    }
    if let Some((w, h)) = std::env::var("M3_GAMES_WINDOW_SIZE").ok().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
    }) {
        viewport = viewport.with_inner_size([w, h]);
    }
    eframe::run_native(
        "material_3_games_overlay",
        eframe::NativeOptions { viewport, ..Default::default() },
        Box::new(|cc| Ok(Box::new(Overlay::new(cc)))),
    )
}
