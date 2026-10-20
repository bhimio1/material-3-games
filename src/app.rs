//! App state, theme, library scanning, launching and action dispatch. The
//! same `App` drives the desktop window and the big-picture mode.

use crate::config::{ColorScheme, Config, ThemeSource};
use crate::hypr;
use crate::launch::{self, Context};
use crate::library::{self, Game};
use crate::session;
use crate::sys::task::Task;
use crate::sys::{cmd, term};
use crate::theme::{sources, style, M3Palette};
use crate::thumbs::Thumbs;
use crate::ui::toasts::{Toast, Toasts};
use crate::ui::{self, bigpicture, pages, widgets, Action, Actions, Filter, Page, ToastKind};
use egui::{CornerRadius, Frame, Margin};
use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Recent,
    Name,
    Playtime,
}

/// A game started from big picture, which we wait on.
pub struct RunningChild {
    pub id: String,
    pub name: String,
    pub child: Child,
    pub started: Instant,
    pub started_at: i64,
}

pub struct App {
    pub ctx: egui::Context,
    pub palette: M3Palette,
    pub config: Config,
    pub games: Vec<Game>,
    scan: Task<Vec<Game>>,
    covers: Task<usize>,
    pub thumbs: Thumbs,
    pub page: Page,
    history: Vec<Page>,
    pub query: String,
    pub sort: Sort,
    pub toasts: Toasts,
    pub actions: Actions,
    pub pages: pages::States,
    /// Big-picture state when running in that mode.
    pub big: Option<bigpicture::State>,
    pub running: Option<RunningChild>,
    /// Windows that will close, while the Game Mode confirmation is open.
    pub confirm_game_mode: Option<Vec<hypr::Client>>,
    entering: Task<Result<session::Entered, String>>,
    rules_job: Task<Result<bool, String>>,
    pub rules_installed: bool,
    system_dark: Option<bool>,
    _watcher: Option<notify::RecommendedWatcher>,
    events: flume::Receiver<PathBuf>,
    rescan_at: Option<Instant>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, big_picture: bool) -> Self {
        let ctx = cc.egui_ctx.clone();
        style::install_fonts(&ctx);
        let config = Config::load();
        let home = crate::sys::paths::home();

        // One watcher for the theme file and the game sources.
        let (tx, events) = flume::unbounded();
        let watcher = {
            use notify::Watcher;
            let ctx = ctx.clone();
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    if ev.kind.is_create() || ev.kind.is_modify() || ev.kind.is_remove() {
                        for p in ev.paths {
                            let _ = tx.send(p);
                        }
                        ctx.request_repaint_after(Duration::from_millis(200));
                    }
                }
            })
            .ok()
            .map(|mut w| {
                let mut dirs = library::watch_dirs(&home);
                if let Some(d) = sources::end4_colors_path().and_then(|p| p.parent().map(PathBuf::from)) {
                    dirs.push(d);
                }
                for d in dirs.into_iter().filter(|d| d.is_dir()) {
                    let _ = w.watch(&d, notify::RecursiveMode::NonRecursive);
                }
                w
            })
        };

        let rules_installed = hypr::rules_installed(&hypr::target(&home));
        let mut app = Self {
            palette: M3Palette::from_seed(crate::theme::palette::DEFAULT_SEED, true),
            games: Vec::new(),
            scan: Task::default(),
            covers: Task::default(),
            thumbs: Thumbs::new(ctx.clone()),
            page: Page::Library(Filter::All),
            history: Vec::new(),
            query: String::new(),
            sort: Sort::Recent,
            toasts: Toasts::default(),
            actions: Vec::new(),
            pages: pages::States::default(),
            big: big_picture.then(|| bigpicture::State::new(ctx.clone())),
            running: None,
            confirm_game_mode: None,
            entering: Task::default(),
            rules_job: Task::default(),
            rules_installed,
            system_dark: ctx.system_theme().map(|t| t == egui::Theme::Dark),
            _watcher: watcher,
            events,
            rescan_at: None,
            config,
            ctx,
        };
        app.apply_theme();
        app.rescan();
        if let Some(err) = session::take_error() {
            app.toast(err, ToastKind::Error);
        }
        if let Ok(page) = std::env::var("M3_GAMES_PAGE") {
            app.page = match page.split_once(':') {
                Some(("detail", id)) => Page::Detail(id.to_string()),
                _ => match page.as_str() {
                    "gamemode" => Page::GameMode,
                    "settings" => Page::Settings,
                    "add" => Page::Add(None),
                    _ => Page::Library(Filter::All),
                },
            };
        }
        app
    }

    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toasts.push(Toast::new(text, kind));
    }

    pub fn is_big_picture(&self) -> bool {
        self.big.is_some()
    }

    pub fn game(&self, id: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.id == id)
    }

    pub fn scanning(&self) -> bool {
        self.scan.running()
    }

    pub fn fetching_covers(&self) -> bool {
        self.covers.running()
    }

    /// Games for the current filter and search, in the chosen order.
    pub fn visible(&self, filter: Filter) -> Vec<&Game> {
        let q = self.query.trim().to_lowercase();
        let mut v: Vec<&Game> = self
            .games
            .iter()
            .filter(|g| filter.matches(g))
            .filter(|g| q.is_empty() || g.name.to_lowercase().contains(&q))
            .collect();
        match self.sort {
            Sort::Recent => {}
            Sort::Name => v.sort_by_key(|g| g.name.to_lowercase()),
            Sort::Playtime => v.sort_by_key(|g| std::cmp::Reverse(g.playtime_minutes.unwrap_or(0))),
        }
        v
    }

    pub fn rescan(&mut self) {
        let enabled = self.config.enabled();
        let home = crate::sys::paths::home();
        self.scan.spawn(&self.ctx, move || {
            let mut games = library::scan(&home, enabled);
            crate::covers::apply_cached(&mut games);
            library::write_cache(&games);
            games
        });
    }

    // ------------------------------------------------------------------ theme

    fn wants_dark(&self) -> bool {
        match self.config.color_scheme {
            ColorScheme::Light => false,
            ColorScheme::Dark => true,
            ColorScheme::System => self.system_dark.unwrap_or(true),
        }
    }

    pub fn theme_file(&self) -> Option<PathBuf> {
        match self.config.theme_source {
            ThemeSource::Auto => sources::auto_theme_path(),
            ThemeSource::Seed => None,
        }
    }

    pub fn apply_theme(&mut self) {
        let dark = self.wants_dark();
        let palette = match self.theme_file() {
            Some(path) => M3Palette::from_theme_json(&path).unwrap_or_else(|_| M3Palette::from_seed(&self.config.seed_color, dark)),
            None => M3Palette::from_seed(&self.config.seed_color, dark),
        };
        style::apply(&self.ctx, &palette);
        widgets::set_palette(&self.ctx, palette.clone());
        widgets::set_filled_icons(self.config.filled_icons);
        // Big picture is read from the couch: everything bigger.
        let scale = self.config.ui_scale.clamp(0.6, 2.5) * if self.is_big_picture() { 1.4 } else { 1.0 };
        if (self.ctx.zoom_factor() - scale).abs() > f32::EPSILON {
            self.ctx.set_zoom_factor(scale);
        }
        self.palette = palette;
    }

    // ------------------------------------------------------------- background

    fn poll(&mut self) {
        self.thumbs.poll();
        if self.scan.poll() {
            if let Some(games) = self.scan.value.take() {
                let missing = games.iter().any(|g| g.cover.is_none() && (g.cover_url.is_some() || !self.config.steamgriddb_key.is_empty()));
                self.games = games;
                if missing && !self.covers.running() && self.covers.finished_at.is_none() {
                    self.fetch_covers();
                }
            }
        }
        if self.covers.poll() {
            if let Some(n) = self.covers.value.take() {
                if n > 0 {
                    self.rescan();
                }
            }
        }
        if self.entering.poll() {
            match self.entering.value.take() {
                Some(Ok(session::Entered::Nested)) => self.toast("Game Mode started in gamescope", ToastKind::Info),
                Some(Ok(session::Entered::Session)) => {}
                Some(Err(e)) => self.toast(e, ToastKind::Error),
                None => {}
            }
        }
        if self.rules_job.poll() {
            match self.rules_job.value.take() {
                Some(Ok(installed)) => {
                    self.rules_installed = installed;
                    self.toast(
                        if installed { "Game rules added to your Hyprland config" } else { "Game rules removed" },
                        ToastKind::Success,
                    );
                }
                Some(Err(e)) => self.toast(format!("Couldn't update Hyprland rules: {e}"), ToastKind::Error),
                None => {}
            }
        }

        let changed: Vec<PathBuf> = self.events.try_iter().collect();
        if !changed.is_empty() {
            let theme = self.theme_file();
            if changed.iter().any(|p| Some(p) == theme.as_ref()) {
                self.apply_theme();
            }
            if changed.iter().any(|p| Some(p) != theme.as_ref() && !p.starts_with(library::cache_dir())) {
                self.rescan_at = Some(Instant::now() + Duration::from_secs(2));
            }
        }
        if self.rescan_at.is_some_and(|t| Instant::now() >= t) && !self.scan.running() {
            self.rescan_at = None;
            self.rescan();
        }
        if let Some(t) = self.rescan_at {
            self.ctx.request_repaint_after(t.saturating_duration_since(Instant::now()));
        }

        // A game started from big picture finished.
        if let Some(r) = &mut self.running {
            if let Ok(Some(_)) = r.child.try_wait() {
                let minutes = (r.started.elapsed().as_secs() + 30) / 60;
                library::record_play(&r.id, r.started_at, minutes);
                session::clear_running(&r.id);
                self.running = None;
                self.rescan();
            } else {
                self.ctx.request_repaint_after(Duration::from_secs(1));
            }
        }

        let system_dark = self.ctx.system_theme().map(|t| t == egui::Theme::Dark);
        if system_dark != self.system_dark {
            self.system_dark = system_dark;
            self.apply_theme();
        }
    }

    // ---------------------------------------------------------------- actions

    fn launch(&mut self, id: &str) {
        let Some(game) = self.game(id).cloned() else {
            self.toast("That game is no longer in your library", ToastKind::Error);
            return;
        };
        if !game.installed {
            self.toast(format!("{} isn't installed", game.name), ToastKind::Error);
            return;
        }
        if self.is_big_picture() {
            let opts = self.config.options(&game.id);
            let plan = launch::plan(&game, &opts, &self.config, Context::BigPicture, None);
            match launch::spawn(&plan) {
                Ok(child) => {
                    session::write_running(&game.id, &game.name, child.id());
                    library::record_play(&game.id, launch::now(), 0);
                    self.running = Some(RunningChild {
                        id: game.id.clone(),
                        name: game.name.clone(),
                        child,
                        started: Instant::now(),
                        started_at: launch::now(),
                    });
                    self.toast(format!("Starting {}", game.name), ToastKind::Info);
                }
                Err(e) => self.toast(e, ToastKind::Error),
            }
        } else {
            match launch::launch_desktop(&game, &self.config) {
                Ok(_) => self.toast(format!("Starting {}", game.name), ToastKind::Info),
                Err(e) => self.toast(e, ToastKind::Error),
            }
        }
    }

    fn fetch_covers(&mut self) {
        let games = self.games.clone();
        let key = self.config.steamgriddb_key.clone();
        self.covers.spawn(&self.ctx, move || crate::covers::fetch_missing(&games, &key));
    }

    fn process(&mut self, action: Action) {
        match action {
            Action::Goto(page) => {
                if page != self.page {
                    let old = std::mem::replace(&mut self.page, page);
                    self.history.push(old);
                    if self.history.len() > 50 {
                        self.history.remove(0);
                    }
                }
            }
            Action::Back => {
                if let Some(p) = self.history.pop() {
                    self.page = p;
                }
            }
            Action::Toast(t, k) => self.toast(t, k),
            Action::Launch(id) => self.launch(&id),
            Action::Install { packages, aur } => {
                let list = packages.iter().map(|p| term::shell_quote(p)).collect::<Vec<_>>().join(" ");
                let script = if aur {
                    let helper = ["paru", "yay"].into_iter().find(|h| cmd::exists(h)).unwrap_or("paru");
                    format!("{helper} -S --needed {list}")
                } else {
                    format!("sudo pacman -S --needed {list}")
                };
                match term::run(&self.config.terminal, &script) {
                    Ok(t) => self.toast(format!("Installing in {t}"), ToastKind::Info),
                    Err(e) => self.toast(e, ToastKind::Error),
                }
            }
            Action::OpenPath(path) => {
                let s = path.to_string_lossy().into_owned();
                let opened = cmd::exists("m3fm") && cmd::spawn_detached("m3fm", &[&s]).is_ok();
                if !opened {
                    if let Err(e) = cmd::spawn_detached("xdg-open", &[&s]) {
                        self.toast(e, ToastKind::Error);
                    }
                }
            }
            Action::OpenUrl(url) => {
                if let Err(e) = cmd::spawn_detached("xdg-open", &[&url]) {
                    self.toast(e, ToastKind::Error);
                }
            }
            Action::Rescan => self.rescan(),
            Action::FetchCovers => self.fetch_covers(),
            Action::ConfigChanged => {
                if let Err(e) = self.config.save() {
                    self.toast(format!("Couldn't save settings: {e}"), ToastKind::Error);
                }
                self.apply_theme();
            }
            Action::ConfirmGameMode => {
                if session::session_ready() && hypr::running() {
                    let me = std::process::id();
                    self.confirm_game_mode = Some(hypr::clients().into_iter().filter(|c| c.pid != me).collect());
                } else {
                    self.process(Action::EnterGameMode);
                }
            }
            Action::EnterGameMode => {
                self.confirm_game_mode = None;
                let config = self.config.clone();
                self.entering.spawn(&self.ctx, move || session::enter(&config));
            }
            Action::InstallRules(install) => {
                let target = hypr::target(&crate::sys::paths::home());
                let learned = self.config.learned_classes.clone();
                self.rules_job.spawn(&self.ctx, move || hypr::write_rules(&target, &learned, install).map(|_| install));
            }
            Action::InstallSession => match session::install_session() {
                Ok(()) => self.toast("Installed. Pick \"Hyprland (with Game Mode)\" at your next login.", ToastKind::Success),
                Err(e) => self.toast(format!("Couldn't install the session: {e}"), ToastKind::Error),
            },
            Action::Quit => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn process_actions(&mut self) {
        while !self.actions.is_empty() {
            for a in std::mem::take(&mut self.actions) {
                self.process(a);
            }
        }
    }

    fn layout(&mut self, ui: &mut egui::Ui) {
        let p = self.palette.clone();
        let surface = Frame::new().fill(p.surface);
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(248.0)
            .size_range(210.0..=320.0)
            .show_separator_line(false)
            .frame(surface.inner_margin(Margin { left: 12, right: 4, top: 12, bottom: 12 }))
            .show(ui, |ui| ui::sidebar::show(ui, self));
        egui::CentralPanel::default()
            .frame(surface.inner_margin(Margin { left: 8, right: 12, top: 12, bottom: 12 }))
            .show(ui, |ui| {
                Frame::new()
                    .fill(p.surface_container_low)
                    .corner_radius(CornerRadius::same(24))
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        pages::show(ui, self);
                    });
            });
        pages::gamemode::confirm_dialog(&self.ctx.clone(), self);
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.is_big_picture() {
            bigpicture::show(ui, self);
        } else {
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::R)) {
                self.actions.push(Action::Rescan);
            }
            if ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Extra1)) {
                self.actions.push(Action::Back);
            }
            self.layout(ui);
        }
        ui::toasts::show(&self.ctx.clone(), self);
        self.process_actions();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette.surface.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.config.save();
    }
}
