//! One game: hero art, Play, stats, and per-game launch options.

use crate::app::App;
use crate::library::{Launch, Source};
use crate::ui::widgets::{self, switch, ButtonKind};
use crate::ui::{art, common, format_last_played, format_playtime, Action, Page};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, Mesh, Rect, RichText, Ui};
use egui_material_icons::icons::*;

#[derive(Default)]
pub struct State {
    /// (game id, env text being edited)
    env: Option<(String, String)>,
    confirm_remove: bool,
}

/// Darken the bottom of the hero so text on it stays readable.
fn scrim(ui: &Ui, rect: Rect, color: Color32) {
    let mut mesh = Mesh::default();
    let top = Color32::TRANSPARENT;
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), color);
    mesh.colored_vertex(rect.right_bottom(), color);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    ui.painter().add(mesh);
}

pub fn show(ui: &mut Ui, app: &mut App, id: &str) {
    let p = app.palette.clone();
    let mut actions = Vec::new();
    let Some(game) = app.game(id).cloned() else {
        common::page(ui, ICON_SPORTS_ESPORTS, "Game", "", |ui| {
            if app.scanning() {
                common::loading(ui, "Loading your library…");
            } else {
                common::empty_hint(ui, "This game isn't in your library any more.");
            }
        });
        return;
    };
    let mut opts = app.config.options(&game.id);
    let mut changed = false;
    let now = crate::launch::now();

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Hero banner.
        let width = ui.available_width();
        let (hero_rect, _) = ui.allocate_exact_size(vec2(width, 300.0), egui::Sense::hover());
        let banner = hero_rect.shrink(12.0);
        let image = game.hero.as_deref().or(game.cover.as_deref());
        if image.is_some() {
            art::paint(ui, &mut app.thumbs, image, banner, 20, ICON_SPORTS_ESPORTS);
        } else {
            ui.painter().rect_filled(banner, CornerRadius::same(20), crate::thumbs::placeholder_color(&game.name, &p));
        }
        scrim(ui, Rect::from_min_max(pos2(banner.left(), banner.center().y - 40.0), banner.max), Color32::from_black_alpha(200));
        // Back button over the art.
        let back = Rect::from_min_size(banner.min + vec2(14.0, 14.0), vec2(40.0, 40.0));
        let b = ui.interact(back, ui.scope_id().with("back"), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.painter().circle_filled(back.center(), 20.0, Color32::from_black_alpha(if b.hovered() { 150 } else { 110 }));
        widgets::paint_icon(ui, ICON_ARROW_BACK, back.center(), 22.0, Color32::WHITE);
        if b.clicked() {
            actions.push(Action::Back);
        }
        // Small cover + title over the bottom of the banner.
        let mut text_left = banner.left() + 28.0;
        if game.hero.is_some() && game.cover.is_some() {
            let c = Rect::from_min_size(pos2(banner.left() + 24.0, banner.bottom() - 24.0 - 150.0), vec2(100.0, 150.0));
            art::paint(ui, &mut app.thumbs, game.cover.as_deref(), c, 12, ICON_SPORTS_ESPORTS);
            text_left = c.right() + 20.0;
        }
        ui.painter().text(
            pos2(text_left, banner.bottom() - 66.0),
            Align2::LEFT_BOTTOM,
            &game.name,
            widgets::title_font(32.0),
            Color32::WHITE,
        );
        ui.painter().text(
            pos2(text_left, banner.bottom() - 40.0),
            Align2::LEFT_BOTTOM,
            format!("{} · {}", game.source.label(), if game.installed { "Installed" } else { "Not installed" }),
            egui::FontId::proportional(14.0),
            Color32::from_white_alpha(220),
        );

        egui::Frame::new().inner_margin(egui::Margin::symmetric(32, 8)).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(common::PAGE_WIDTH));
            ui.horizontal(|ui| {
                if widgets::button_enabled(ui, ButtonKind::Filled, Some(ICON_PLAY_ARROW), "Play", game.installed).clicked() {
                    actions.push(Action::Launch(game.id.clone()));
                }
                if let Some(dir) = game.install_dir.clone().filter(|d| d.is_dir()) {
                    if widgets::button(ui, ButtonKind::Tonal, Some(ICON_FOLDER_OPEN), "Open folder").clicked() {
                        actions.push(Action::OpenPath(dir));
                    }
                }
                if let Launch::Manual(m) = &game.launch {
                    if widgets::button(ui, ButtonKind::Text, Some(ICON_EDIT), "Edit").clicked() {
                        actions.push(Action::Goto(Page::Add(Some(m.slug.clone()))));
                    }
                    if widgets::button(ui, ButtonKind::Text, Some(ICON_DELETE), "Remove").clicked() {
                        app.pages.detail.confirm_remove = true;
                    }
                }
            });
            common::card(ui, "Activity", |ui| {
                common::facts(
                    ui,
                    "activity",
                    &[
                        ("Play time", format_playtime(game.playtime_minutes)),
                        ("Last played", format_last_played(game.last_played, now)),
                        (
                            "Install folder",
                            game.install_dir.as_deref().map(crate::sys::paths::tilde).unwrap_or_else(|| "—".into()),
                        ),
                        ("Id", game.id.clone()),
                    ],
                );
            });

            let store = matches!(game.launch, Launch::Steam { .. } | Launch::Lutris { .. } | Launch::Heroic { .. });
            common::card(ui, "Launch options", |ui| {
                if store {
                    let hint = match game.source {
                        Source::Steam => "Steam starts this game, so GameMode, MangoHud and gamescope go in its Steam launch options instead (e.g. gamemoderun mangohud %command%).",
                        Source::Lutris => "Lutris starts this game; set GameMode, MangoHud and gamescope in its Lutris system options.",
                        _ => "Heroic starts this game; set GameMode, MangoHud and gamescope in its Heroic settings.",
                    };
                    common::note(ui, hint);
                    ui.separator();
                } else {
                    common::row(ui, "GameMode", "Ask Feral GameMode to optimise the CPU governor and scheduling while playing.", |ui| {
                        changed |= switch(ui, &mut opts.gamemode).changed();
                    });
                    if opts.gamemode {
                        common::require(ui, &mut actions, "gamemoderun", "gamemode", "");
                    }
                    ui.separator();
                    common::row(ui, "MangoHud", "Frame rate, frame times and temperatures overlay.", |ui| {
                        changed |= switch(ui, &mut opts.mangohud).changed();
                    });
                    if opts.mangohud {
                        common::require(ui, &mut actions, "mangohud", "mangohud", "");
                    }
                    ui.separator();
                    common::row(ui, "Run in gamescope", "On the desktop too, with the resolution and upscaling from Settings. Game Mode always uses gamescope.", |ui| {
                        changed |= switch(ui, &mut opts.gamescope).changed();
                    });
                    if opts.gamescope {
                        common::require(ui, &mut actions, "gamescope", "gamescope", "");
                    }
                    ui.separator();
                }
                common::row(ui, "Fullscreen on Hyprland", "When the game's window appears, focus it and make it fullscreen, with tearing allowed and animations off.", |ui| {
                    changed |= switch(ui, &mut opts.force_fullscreen).changed();
                });
                ui.separator();
                let state = &mut app.pages.detail;
                if state.env.as_ref().map(|(i, _)| i.as_str()) != Some(game.id.as_str()) {
                    state.env = Some((game.id.clone(), opts.env.join("\n")));
                }
                let text = &mut state.env.as_mut().expect("set above").1;
                common::row(ui, "Environment", "One KEY=value per line, e.g. DXVK_HUD=fps or PROTON_ENABLE_WAYLAND=1.", |_| {});
                let r = ui.add(egui::TextEdit::multiline(text).code_editor().desired_rows(3).desired_width(f32::INFINITY).hint_text("KEY=value"));
                if r.lost_focus() {
                    opts.env = text.lines().map(str::trim).filter(|l| l.contains('=')).map(str::to_string).collect();
                    changed = true;
                }
                ui.add_space(8.0);
            });
        });
    });

    if app.pages.detail.confirm_remove {
        let mut close = false;
        let r = common::modal(&app.ctx.clone(), &p, "remove", ICON_DELETE, &format!("Remove {}?", game.name), |ui| {
            ui.label(RichText::new("It's removed from this list only. Its files stay where they are.").color(p.on_surface_variant));
            common::dialog_buttons(ui, |ui| {
                if widgets::button(ui, ButtonKind::Danger, None, "Remove").clicked() {
                    if let Launch::Manual(m) = &game.launch {
                        match crate::library::manual::remove(&m.slug) {
                            Ok(()) => {
                                actions.push(Action::Rescan);
                                actions.push(Action::Goto(Page::Library(crate::ui::Filter::All)));
                            }
                            Err(e) => actions.push(Action::Toast(e.to_string(), crate::ui::ToastKind::Error)),
                        }
                    }
                    close = true;
                }
                if widgets::button(ui, ButtonKind::Text, None, "Cancel").clicked() {
                    close = true;
                }
            });
        });
        if close || r.should_close() {
            app.pages.detail.confirm_remove = false;
        }
    }

    if changed {
        app.config.games.insert(game.id.clone(), opts);
        actions.push(Action::ConfigChanged);
    }
    app.actions.extend(actions);
}
