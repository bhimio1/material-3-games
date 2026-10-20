//! Big picture: the full-screen, controller-first library shown in Game
//! Mode. A "Continue playing" row of wide hero cards, then every installed
//! game as cover tiles. Focus moves with the D-pad, stick or arrow keys.

use super::pages::library::{cover_card, ASPECT};
use super::widgets::{self, ButtonKind};
use super::{art, format_last_played, format_playtime, Action, ToastKind};
use crate::app::App;
use crate::gamepad::{self, Nav};
use crate::library::Game;
use crate::sys::cmd;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, Frame, Margin, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use egui_material_icons::icons::*;

#[derive(Clone, Debug, PartialEq)]
enum Overlay {
    None,
    /// Game options for this id; selected item.
    Detail(String, usize),
    Menu(usize),
    ConfirmPower(usize),
}

pub struct State {
    pad: Option<flume::Receiver<Nav>>,
    /// Row 0 = "Continue playing", then grid rows.
    row: usize,
    col: usize,
    overlay: Overlay,
    scroll_to_focus: bool,
}

impl State {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            pad: gamepad::start(ctx),
            row: 0,
            col: 0,
            overlay: Overlay::None,
            scroll_to_focus: true,
        }
    }
}

const MENU: [(&str, egui_material_icons::MaterialIcon); 7] = [
    ("Resume", ICON_PLAY_ARROW),
    ("Volume up", ICON_VOLUME_UP),
    ("Volume down", ICON_VOLUME_DOWN),
    ("Steam Big Picture", ICON_STADIA_CONTROLLER),
    ("Exit to desktop", ICON_LOGOUT),
    ("Sleep", ICON_BEDTIME),
    ("Power off", ICON_POWER_SETTINGS_NEW),
];

fn keyboard_nav(ui: &Ui) -> Vec<Nav> {
    ui.input(|i| {
        let mut v = Vec::new();
        let map = [
            (egui::Key::ArrowUp, Nav::Up),
            (egui::Key::ArrowDown, Nav::Down),
            (egui::Key::ArrowLeft, Nav::Left),
            (egui::Key::ArrowRight, Nav::Right),
            (egui::Key::Enter, Nav::Accept),
            (egui::Key::Space, Nav::Accept),
            (egui::Key::Escape, Nav::Back),
            (egui::Key::Backspace, Nav::Back),
            (egui::Key::Tab, Nav::Options),
            (egui::Key::F1, Nav::Menu),
        ];
        for (k, n) in map {
            if i.key_pressed(k) {
                v.push(n);
            }
        }
        v
    })
}

fn battery() -> Option<u8> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    dir.flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("BAT"))
        .find_map(|e| std::fs::read_to_string(e.path().join("capacity")).ok()?.trim().parse().ok())
}

fn online() -> bool {
    std::fs::read_dir("/sys/class/net").is_ok_and(|d| {
        d.flatten().any(|e| {
            e.file_name() != "lo" && std::fs::read_to_string(e.path().join("operstate")).is_ok_and(|s| s.trim() == "up")
        })
    })
}

fn volume(delta: &str) {
    let _ = cmd::spawn_detached("pactl", &["set-sink-volume", "@DEFAULT_SINK@", delta]);
}

fn top_bar(ui: &mut Ui, app: &App) {
    let p = app.palette.clone();
    ui.horizontal(|ui| {
        widgets::icon(ui, ICON_SPORTS_ESPORTS, 30.0, p.primary);
        ui.label(widgets::title_text("Game Mode", 22.0).color(p.on_surface));
        if let Some(r) = &app.running {
            ui.add_space(16.0);
            Frame::new().fill(p.tertiary_container).corner_radius(CornerRadius::same(14)).inner_margin(Margin::symmetric(12, 4)).show(ui, |ui| {
                ui.label(RichText::new(format!("Playing {}", r.name)).color(p.on_tertiary_container));
            });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let now = chrono::Local::now();
            ui.label(widgets::title_text(now.format("%H:%M").to_string(), 22.0).color(p.on_surface));
            ui.add_space(12.0);
            if let Some(b) = battery() {
                ui.label(RichText::new(format!("{b}%")).size(16.0).color(p.on_surface_variant));
                widgets::icon(ui, ICON_BATTERY_FULL, 22.0, p.on_surface_variant);
                ui.add_space(8.0);
            }
            widgets::icon(ui, if online() { ICON_WIFI } else { ICON_WIFI_OFF }, 22.0, p.on_surface_variant);
            ui.add_space(12.0);
            ui.label(RichText::new("Start  Menu      Y  Options      A  Play").size(14.0).color(p.on_surface_variant));
        });
    });
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(20));
}

/// A wide hero card for the "Continue playing" row.
fn hero_card(ui: &mut Ui, app: &mut App, game: &Game, size: egui::Vec2, focused: bool) -> egui::Response {
    let p = app.palette.clone();
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let img = game.hero.as_deref().or(game.cover.as_deref());
        if img.is_some() {
            art::paint(ui, &mut app.thumbs, img, rect, 20, ICON_SPORTS_ESPORTS);
        } else {
            ui.painter().rect_filled(rect, CornerRadius::same(20), crate::thumbs::placeholder_color(&game.name, &p));
        }
        let band = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 64.0), rect.max);
        ui.painter().rect_filled(
            band,
            CornerRadius { nw: 0, ne: 0, sw: 20, se: 20 },
            Color32::from_black_alpha(150),
        );
        let title = widgets::truncated_galley(ui, &game.name, widgets::title_font(20.0), Color32::WHITE, rect.width() - 32.0);
        ui.painter().galley(pos2(rect.left() + 16.0, band.top() + 10.0), title, Color32::WHITE);
        ui.painter().text(
            pos2(rect.left() + 16.0, band.bottom() - 10.0),
            Align2::LEFT_BOTTOM,
            format!("{} · {}", format_last_played(game.last_played, crate::launch::now()), format_playtime(game.playtime_minutes)),
            egui::FontId::proportional(13.0),
            Color32::from_white_alpha(210),
        );
        if focused {
            ui.painter().rect_stroke(rect.expand(4.0), CornerRadius::same(24), Stroke::new(4.0, p.primary), StrokeKind::Outside);
        } else if resp.hovered() {
            ui.painter().rect_stroke(rect.expand(3.0), CornerRadius::same(23), Stroke::new(2.0, p.outline), StrokeKind::Outside);
        }
    }
    resp
}

fn menu_list(ui: &mut Ui, items: &[(&str, egui_material_icons::MaterialIcon)], selected: usize) -> Option<usize> {
    let p = widgets::palette(ui);
    let mut clicked = None;
    for (i, (label, icon)) in items.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
        let sel = i == selected;
        let fill = if sel {
            p.secondary_container
        } else if resp.hovered() {
            p.on_surface.gamma_multiply(0.08)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(26), fill);
        let fg = if sel { p.on_secondary_container } else { p.on_surface };
        widgets::paint_icon(ui, *icon, pos2(rect.left() + 30.0, rect.center().y), 24.0, fg);
        ui.painter().text(pos2(rect.left() + 58.0, rect.center().y), Align2::LEFT_CENTER, *label, widgets::title_font(17.0), fg);
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

fn overlay_frame<R>(ui: &mut Ui, p: &crate::theme::M3Palette, width: f32, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let screen = ui.ctx().content_rect();
    ui.painter().rect_filled(screen, CornerRadius::ZERO, p.scrim.gamma_multiply(0.6));
    egui::Area::new(egui::Id::unique("m3g-overlay"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(p.surface_container_high)
                .corner_radius(CornerRadius::same(28))
                .inner_margin(Margin::same(24))
                .show(ui, |ui| {
                    ui.set_width(width);
                    add(ui)
                })
                .inner
        })
        .inner
        .into()
}

pub fn show(ui: &mut Ui, app: &mut App) {
    let p = app.palette.clone();
    let mut actions: Vec<Action> = Vec::new();
    let Some(mut st) = app.big.take() else {
        return;
    };
    let mut navs = keyboard_nav(ui);
    if let Some(rx) = &st.pad {
        navs.extend(rx.try_iter());
    }

    let recent: Vec<Game> = app.games.iter().filter(|g| g.installed && g.last_played.is_some()).take(8).cloned().collect();
    let mut all: Vec<Game> = app.games.iter().filter(|g| g.installed).cloned().collect();
    all.sort_by_key(|g| g.name.to_lowercase());

    let width = ui.available_width() - 96.0;
    let gap = 28.0;
    let cols = ((width + gap) / (200.0 + gap)).floor().max(3.0) as usize;
    let tile_w = ((width - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let has_recent = !recent.is_empty();
    let grid_rows = all.len().div_ceil(cols);
    let rows = usize::from(has_recent) + grid_rows;
    let row_len = |r: usize| -> usize {
        if has_recent && r == 0 {
            recent.len()
        } else {
            let gr = r - usize::from(has_recent);
            all.len().saturating_sub(gr * cols).min(cols)
        }
    };
    let focused_game = |row: usize, col: usize| -> Option<Game> {
        if has_recent && row == 0 {
            recent.get(col).cloned()
        } else {
            all.get((row - usize::from(has_recent)) * cols + col).cloned()
        }
    };

    // ---------------------------------------------------------- navigation
    for nav in navs {
        match st.overlay.clone() {
            Overlay::None => match nav {
                Nav::Up if st.row > 0 => {
                    st.row -= 1;
                    st.col = st.col.min(row_len(st.row).saturating_sub(1));
                    st.scroll_to_focus = true;
                }
                Nav::Down if st.row + 1 < rows => {
                    st.row += 1;
                    st.col = st.col.min(row_len(st.row).saturating_sub(1));
                    st.scroll_to_focus = true;
                }
                Nav::Left if st.col > 0 => {
                    st.col -= 1;
                    st.scroll_to_focus = true;
                }
                Nav::Right if st.col + 1 < row_len(st.row) => {
                    st.col += 1;
                    st.scroll_to_focus = true;
                }
                Nav::Accept => {
                    if let Some(g) = focused_game(st.row, st.col) {
                        actions.push(Action::Launch(g.id));
                    }
                }
                Nav::Options => {
                    if let Some(g) = focused_game(st.row, st.col) {
                        st.overlay = Overlay::Detail(g.id, 0);
                    }
                }
                Nav::Menu | Nav::Back => st.overlay = Overlay::Menu(0),
                _ => {}
            },
            Overlay::Detail(id, sel) => match nav {
                Nav::Up => st.overlay = Overlay::Detail(id, sel.saturating_sub(1)),
                Nav::Down => st.overlay = Overlay::Detail(id, (sel + 1).min(3)),
                Nav::Back | Nav::Options => st.overlay = Overlay::None,
                Nav::Accept => detail_activate(app, &mut st, &id, sel, &mut actions),
                _ => {}
            },
            Overlay::Menu(sel) => match nav {
                Nav::Up => st.overlay = Overlay::Menu(sel.saturating_sub(1)),
                Nav::Down => st.overlay = Overlay::Menu((sel + 1).min(MENU.len() - 1)),
                Nav::Back | Nav::Menu => st.overlay = Overlay::None,
                Nav::Accept => menu_activate(&mut st, sel, &mut actions),
                _ => {}
            },
            Overlay::ConfirmPower(sel) => match nav {
                Nav::Left | Nav::Right | Nav::Up | Nav::Down => st.overlay = Overlay::ConfirmPower(1 - sel.min(1)),
                Nav::Back => st.overlay = Overlay::Menu(6),
                Nav::Accept => {
                    if sel == 0 {
                        let _ = cmd::spawn_detached("systemctl", &["poweroff"]);
                    }
                    st.overlay = Overlay::Menu(6);
                }
                _ => {}
            },
        }
    }
    st.row = st.row.min(rows.saturating_sub(1));

    // -------------------------------------------------------------- layout
    Frame::new().fill(p.surface).inner_margin(Margin::symmetric(48, 28)).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        top_bar(ui, app);
        ui.add_space(20.0);
        if app.games.is_empty() {
            ui.add_space(120.0);
            ui.vertical_centered(|ui| {
                if app.scanning() {
                    ui.add(egui::Spinner::new().size(48.0).color(p.primary));
                    ui.label(RichText::new("Finding your games…").size(20.0).color(p.on_surface_variant));
                } else {
                    ui.label(RichText::new("No games yet. Add some from the desktop app.").size(20.0).color(p.on_surface_variant));
                }
            });
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut grid_row_offset = 0;
            if has_recent {
                ui.label(widgets::title_text("Continue playing", 22.0).color(p.on_surface));
                ui.add_space(12.0);
                let size = vec2((width * 0.42).clamp(360.0, 640.0), (width * 0.42).clamp(360.0, 640.0) * 0.47);
                egui::ScrollArea::horizontal().id_salt("recent").show(ui, |ui| {
                    // Room for the focus ring, which the scroll area would clip.
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.spacing_mut().item_spacing.x = gap;
                        for (i, g) in recent.iter().enumerate() {
                            let focused = st.row == 0 && st.col == i && st.overlay == Overlay::None;
                            let r = hero_card(ui, app, g, size, focused);
                            if focused && st.scroll_to_focus {
                                r.scroll_to_me(None);
                            }
                            if r.clicked() {
                                st.row = 0;
                                st.col = i;
                                actions.push(Action::Launch(g.id.clone()));
                            }
                        }
                    });
                    ui.add_space(8.0);
                });
                ui.add_space(24.0);
                grid_row_offset = 1;
            }
            ui.label(widgets::title_text("Library", 22.0).color(p.on_surface));
            ui.add_space(12.0);
            for (r, chunk) in all.chunks(cols).enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (c, g) in chunk.iter().enumerate() {
                        let focused = st.row == r + grid_row_offset && st.col == c && st.overlay == Overlay::None;
                        let (resp, play) = cover_card(ui, &mut app.thumbs, g, tile_w, focused);
                        if focused && st.scroll_to_focus {
                            resp.scroll_to_me(None);
                        }
                        if play || resp.clicked() {
                            st.row = r + grid_row_offset;
                            st.col = c;
                            actions.push(Action::Launch(g.id.clone()));
                        }
                        if resp.secondary_clicked() {
                            st.overlay = Overlay::Detail(g.id.clone(), 0);
                        }
                    }
                });
                ui.add_space(22.0);
            }
            let _ = ASPECT;
        });
    });
    st.scroll_to_focus = false;

    // ------------------------------------------------------------ overlays
    match st.overlay.clone() {
        Overlay::Detail(id, sel) => {
            if let Some(g) = app.game(&id).cloned() {
                let opts = app.config.options(&id);
                let thumbs = &mut app.thumbs;
                let clicked = overlay_frame(ui, &p, 620.0, |ui| {
                    let banner = ui.allocate_exact_size(vec2(ui.available_width(), 220.0), Sense::hover()).0;
                    art::paint(ui, thumbs, g.hero.as_deref().or(g.cover.as_deref()), banner, 18, ICON_SPORTS_ESPORTS);
                    ui.add_space(14.0);
                    ui.label(widgets::title_text(&g.name, 26.0).color(p.on_surface));
                    ui.label(
                        RichText::new(format!(
                            "{} · Last played {} · {}",
                            g.source.label(),
                            format_last_played(g.last_played, crate::launch::now()).to_lowercase(),
                            format_playtime(g.playtime_minutes)
                        ))
                        .size(15.0)
                        .color(p.on_surface_variant),
                    );
                    ui.add_space(14.0);
                    let check = |on: bool| if on { ICON_CHECK_BOX } else { ICON_CHECK_BOX_OUTLINE_BLANK };
                    let items = [
                        ("Play", ICON_PLAY_ARROW),
                        ("GameMode", check(opts.gamemode)),
                        ("MangoHud", check(opts.mangohud)),
                        ("Close", ICON_CLOSE),
                    ];
                    menu_list(ui, &items, sel)
                })
                .flatten();
                if let Some(i) = clicked {
                    detail_activate(app, &mut st, &id, i, &mut actions);
                }
            } else {
                st.overlay = Overlay::None;
            }
        }
        Overlay::Menu(sel) => {
            let clicked = overlay_frame(ui, &p, 420.0, |ui| {
                ui.label(widgets::title_text("Menu", 24.0).color(p.on_surface));
                ui.add_space(12.0);
                menu_list(ui, &MENU, sel)
            })
            .flatten();
            if let Some(i) = clicked {
                menu_activate(&mut st, i, &mut actions);
            }
        }
        Overlay::ConfirmPower(sel) => {
            overlay_frame(ui, &p, 420.0, |ui| {
                ui.label(widgets::title_text("Power off?", 24.0).color(p.on_surface));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let kinds = [(ButtonKind::Danger, "Power off"), (ButtonKind::Tonal, "Cancel")];
                    for (i, (k, label)) in kinds.into_iter().enumerate() {
                        let kind = if i == sel { k } else { ButtonKind::Outlined };
                        if widgets::button(ui, kind, None, label).clicked() {
                            if i == 0 {
                                let _ = cmd::spawn_detached("systemctl", &["poweroff"]);
                            }
                            st.overlay = Overlay::Menu(6);
                        }
                    }
                });
            });
        }
        Overlay::None => {}
    }

    app.big = Some(st);
    app.actions.extend(actions);
}

fn detail_activate(app: &mut App, st: &mut State, id: &str, sel: usize, actions: &mut Vec<Action>) {
    match sel {
        0 => {
            actions.push(Action::Launch(id.to_string()));
            st.overlay = Overlay::None;
        }
        1 | 2 => {
            let mut o = app.config.options(id);
            if sel == 1 {
                o.gamemode = !o.gamemode;
            } else {
                o.mangohud = !o.mangohud;
            }
            app.config.games.insert(id.to_string(), o);
            actions.push(Action::ConfigChanged);
            st.overlay = Overlay::Detail(id.to_string(), sel);
        }
        _ => st.overlay = Overlay::None,
    }
}

fn menu_activate(st: &mut State, sel: usize, actions: &mut Vec<Action>) {
    match sel {
        0 => st.overlay = Overlay::None,
        1 => volume("+5%"),
        2 => volume("-5%"),
        3 => {
            if cmd::spawn_detached("steam", &["-gamepadui"]).is_err() {
                actions.push(Action::Toast("Steam isn't installed".into(), ToastKind::Error));
            }
            st.overlay = Overlay::None;
        }
        4 => actions.push(Action::Quit),
        5 => {
            let _ = cmd::spawn_detached("systemctl", &["suspend"]);
            st.overlay = Overlay::None;
        }
        _ => st.overlay = Overlay::ConfirmPower(1),
    }
    st.scroll_to_focus = true;
}
