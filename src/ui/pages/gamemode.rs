//! Game Mode: explains full (gamescope on the GPU, Hyprland closed) vs
//! nested (gamescope inside Hyprland), and starts it.

use crate::app::App;
use crate::session;
use crate::sys::cmd;
use crate::ui::widgets::{self, ButtonKind};
use crate::ui::{common, Action, Page};
use egui::{vec2, CornerRadius, Frame, Margin, RichText, Ui};
use egui_material_icons::icons::*;

fn chip(ui: &mut Ui, ok: bool, text: &str) {
    let p = widgets::palette(ui);
    let (bg, fg, icon) = if ok {
        (p.secondary_container, p.on_secondary_container, ICON_CHECK_CIRCLE)
    } else {
        (p.surface_container_highest, p.on_surface_variant, ICON_RADIO_BUTTON_UNCHECKED)
    };
    Frame::new().fill(bg).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(10, 5)).show(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::icon(ui, icon, 16.0, fg);
            ui.label(RichText::new(text).size(13.0).color(fg));
        });
    });
}

pub fn show(ui: &mut Ui, app: &mut App) {
    let p = app.palette.clone();
    let mut actions = Vec::new();
    let ready = session::session_ready();
    let installed = session::session_installed();
    let have_gamescope = cmd::exists("gamescope");
    let on_hyprland = crate::hypr::running();

    common::page(ui, ICON_VIDEOGAME_ASSET, "Game Mode", "A full-screen, controller-friendly library that runs your games in gamescope.", |ui| {
        ui.add_space(16.0);
        // Hero card.
        Frame::new()
            .fill(p.primary_container)
            .corner_radius(CornerRadius::same(28))
            .inner_margin(Margin::same(28))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    widgets::icon(ui, ICON_STADIA_CONTROLLER, 56.0, p.on_primary_container);
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.label(widgets::title_text(if ready { "Full Game Mode" } else { "Nested Game Mode" }, 24.0).color(p.on_primary_container));
                        ui.add(
                            egui::Label::new(
                                RichText::new(if ready {
                                    "Closes your apps, leaves Hyprland and runs gamescope directly on your GPU, so games get every frame. Exiting brings your desktop back."
                                } else {
                                    "Runs gamescope full screen inside Hyprland. Your apps stay open. For the best performance, use full Game Mode below."
                                })
                                .color(p.on_primary_container),
                            )
                            .wrap(),
                        );
                    });
                });
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    if widgets::button_enabled(ui, ButtonKind::Filled, Some(ICON_PLAY_ARROW), "Enter Game Mode", have_gamescope).clicked() {
                        actions.push(Action::ConfirmGameMode);
                    }
                    if widgets::button(ui, ButtonKind::Text, Some(ICON_TUNE), "gamescope settings").clicked() {
                        actions.push(Action::Goto(Page::Settings));
                    }
                });
            });
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            chip(ui, have_gamescope, "gamescope installed");
            chip(ui, installed, "Login session installed");
            chip(ui, ready, "Running in the Game Mode session");
            chip(ui, on_hyprland, "Hyprland");
        });
        if !have_gamescope {
            common::require(ui, &mut actions, "gamescope", "gamescope", "Game Mode runs in Valve's gamescope compositor.");
        }

        common::card(ui, "Full Game Mode", |ui| {
            common::note(
                ui,
                "Hyprland and other desktop compositors cost frames. Full Game Mode works like SteamOS: a login session called \
                 \"Hyprland (with Game Mode)\" starts Hyprland as usual, and when you enter Game Mode it closes your apps, ends \
                 Hyprland and runs gamescope on its own. When you exit, Hyprland starts again.",
            );
            if !installed {
                common::row(ui, "Install the login session", "Adds the session entry and the m3-session script (asks for your password).", |ui| {
                    if widgets::button(ui, ButtonKind::Tonal, Some(ICON_DOWNLOAD), "Install").clicked() {
                        actions.push(Action::InstallSession);
                    }
                });
            } else if !ready {
                common::row(ui, "Log in with \"Hyprland (with Game Mode)\"", "Pick it once in your login screen's session menu. It's Hyprland, plus the switch to Game Mode.", |_| {});
            } else {
                common::row(ui, "Ready", "You're in the Game Mode session. Entering Game Mode will close your apps first.", |ui| {
                    widgets::icon(ui, ICON_CHECK_CIRCLE, 24.0, p.primary);
                });
            }
            ui.add_space(4.0);
        });

        common::card(ui, "In Game Mode", |ui| {
            common::facts(
                ui,
                "controls",
                &[
                    ("A / Enter", "Play or open".into()),
                    ("B / Esc", "Back".into()),
                    ("Y", "Game options".into()),
                    ("Start / Menu", "System menu: volume, Steam Big Picture, exit to desktop, power".into()),
                    ("D-pad / stick / arrows", "Move".into()),
                ],
            );
        });
    });
    app.actions.extend(actions);
}

/// Confirmation listing the windows that will close.
pub fn confirm_dialog(ctx: &egui::Context, app: &mut App) {
    let Some(windows) = app.confirm_game_mode.clone() else {
        return;
    };
    let p = app.palette.clone();
    let mut actions = Vec::new();
    let mut close = false;
    let r = common::modal(ctx, &p, "gamemode", ICON_VIDEOGAME_ASSET, "Enter Game Mode?", |ui| {
        ui.set_max_width(460.0);
        let text = if windows.is_empty() {
            "Hyprland will close and gamescope will start.".to_string()
        } else {
            format!("These {} windows will close. Save your work first.", windows.len())
        };
        ui.label(RichText::new(text).color(p.on_surface_variant));
        if !windows.is_empty() {
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for w in &windows {
                    ui.horizontal(|ui| {
                        widgets::icon(ui, ICON_WEB_ASSET, 18.0, p.on_surface_variant);
                        let label = if w.title.is_empty() { w.class.clone() } else { format!("{} — {}", w.title, w.class) };
                        ui.add(egui::Label::new(RichText::new(label).color(p.on_surface)).truncate());
                    });
                    ui.add_space(2.0);
                }
            });
        }
        common::dialog_buttons(ui, |ui| {
            if widgets::button(ui, ButtonKind::Filled, Some(ICON_PLAY_ARROW), "Enter Game Mode").clicked() {
                actions.push(Action::EnterGameMode);
                close = true;
            }
            if widgets::button(ui, ButtonKind::Text, None, "Cancel").clicked() {
                close = true;
            }
        });
        ui.allocate_space(vec2(0.0, 0.0));
    });
    if close || r.should_close() {
        app.confirm_game_mode = None;
    }
    app.actions.extend(actions);
}
