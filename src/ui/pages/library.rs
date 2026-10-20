//! The library grid: cover cards with search and sorting.

use crate::app::{App, Sort};
use crate::library::Game;
use crate::thumbs::{placeholder_color, Thumbs};
use crate::ui::widgets::{self, segmented};
use crate::ui::{art, common, Action, Filter, Page};
use egui::{pos2, vec2, Align2, CornerRadius, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui};
use egui_material_icons::icons::*;

/// Cover aspect (width / height), Steam's 600x900 portrait.
pub const ASPECT: f32 = 2.0 / 3.0;

/// A portrait cover card with the title underneath. `focused` draws the
/// focus ring used by big picture and keyboard navigation.
pub fn cover_card(ui: &mut Ui, thumbs: &mut Thumbs, game: &Game, width: f32, focused: bool) -> (Response, bool) {
    let p = widgets::palette(ui);
    let cover_h = width / ASPECT;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, cover_h + 46.0), Sense::click());
    let hover = resp.hovered() || focused;
    let t = ui.ctx().animate_bool_with_time(resp.id, hover, 0.12);
    let lift = t * 4.0;
    let cover = Rect::from_min_size(rect.min - vec2(0.0, lift), vec2(width, cover_h));
    let mut play = false;
    if ui.is_rect_visible(rect) {
        if t > 0.0 {
            ui.painter().rect_filled(
                cover.translate(vec2(0.0, 6.0)).shrink(4.0),
                CornerRadius::same(16),
                egui::Color32::from_black_alpha((60.0 * t) as u8),
            );
        }
        if game.cover.is_some() {
            art::paint(ui, thumbs, game.cover.as_deref(), cover, 16, ICON_SPORTS_ESPORTS);
        } else {
            let fill = placeholder_color(&game.name, &p);
            ui.painter().rect_filled(cover, CornerRadius::same(16), fill);
            let initials: String = game.name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
            ui.painter().text(cover.center(), Align2::CENTER_CENTER, initials.to_uppercase(), widgets::title_font(width * 0.22), p.on_primary_container);
        }
        if !game.installed {
            ui.painter().rect_filled(cover, CornerRadius::same(16), p.scrim.gamma_multiply(0.45));
            widgets::paint_icon(ui, ICON_DOWNLOAD, cover.center(), 32.0, egui::Color32::WHITE);
        }
        if focused {
            ui.painter().rect_stroke(cover.expand(3.0), CornerRadius::same(19), Stroke::new(3.0, p.primary), StrokeKind::Outside);
        }
        if hover && game.installed {
            let c = pos2(cover.right() - 30.0, cover.bottom() - 30.0);
            let r = Rect::from_center_size(c, vec2(44.0, 44.0));
            let b = ui.interact(r, resp.id.with("play"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let fill = widgets::state_layer(p.primary, p.on_primary, &b);
            ui.painter().circle_filled(c, 22.0 * (0.85 + 0.15 * t), fill);
            widgets::paint_icon(ui, ICON_PLAY_ARROW, c, 26.0, p.on_primary);
            play = b.clicked();
        }
        let title = widgets::truncated_galley(ui, &game.name, widgets::title_font(14.0), p.on_surface, width - 4.0);
        ui.painter().galley(pos2(rect.left() + 2.0, cover.bottom() + 8.0), title, p.on_surface);
        ui.painter().text(
            pos2(rect.left() + 2.0, cover.bottom() + 28.0),
            Align2::LEFT_TOP,
            game.source.label(),
            egui::FontId::proportional(12.0),
            p.on_surface_variant,
        );
    }
    (resp.on_hover_cursor(egui::CursorIcon::PointingHand), play)
}

pub fn show(ui: &mut Ui, app: &mut App, filter: Filter) {
    let p = app.palette.clone();
    let mut actions = Vec::new();
    let subtitle = if app.scanning() && app.games.is_empty() {
        "Looking for your games…".to_string()
    } else {
        let n = app.games.iter().filter(|g| filter.matches(g)).count();
        format!("{n} game{}", if n == 1 { "" } else { "s" })
    };
    common::page(ui, ICON_SPORTS_ESPORTS, filter.title(), &subtitle, |ui| {
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 340.0).clamp(200.0, 420.0);
            common::search_field(ui, &mut app.query, "Search your games", w);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if common::refresh_button(ui, app.scanning()) {
                    actions.push(Action::Rescan);
                }
                segmented(ui, &mut app.sort, &[(Sort::Recent, ICON_HISTORY, "Recent"), (Sort::Name, ICON_SORT_BY_ALPHA, "Name"), (Sort::Playtime, ICON_TIMER, "Time")]);
            });
        });
        ui.add_space(18.0);

        let games: Vec<Game> = app.visible(filter).into_iter().cloned().collect();
        if games.is_empty() {
            if app.scanning() {
                common::loading(ui, "Scanning Steam, Lutris, Heroic and your apps…");
            } else {
                common::card(ui, "", |ui| {
                    common::empty_hint(ui, if app.query.is_empty() { "No games here yet." } else { "No games match your search." });
                    ui.horizontal(|ui| {
                        if widgets::button(ui, widgets::ButtonKind::Tonal, Some(ICON_ADD), "Add a game").clicked() {
                            actions.push(Action::Goto(Page::Add(None)));
                        }
                    });
                    ui.add_space(8.0);
                });
            }
            return;
        }
        let gap = 20.0;
        let avail = ui.available_width();
        let cols = ((avail + gap) / (160.0 + gap)).floor().max(2.0) as usize;
        let width = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        for row in games.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for g in row {
                    let (resp, play) = cover_card(ui, &mut app.thumbs, g, width, false);
                    if play {
                        actions.push(Action::Launch(g.id.clone()));
                    } else if resp.clicked() {
                        actions.push(Action::Goto(Page::Detail(g.id.clone())));
                    } else if resp.double_clicked() {
                        actions.push(Action::Launch(g.id.clone()));
                    }
                    resp.context_menu(|ui| {
                        if ui.button(RichText::new("Play")).clicked() {
                            actions.push(Action::Launch(g.id.clone()));
                            ui.close();
                        }
                        if ui.button("Details").clicked() {
                            actions.push(Action::Goto(Page::Detail(g.id.clone())));
                            ui.close();
                        }
                    });
                }
            });
            ui.add_space(18.0);
        }
        let _ = p;
    });
    app.actions.extend(actions);
}
