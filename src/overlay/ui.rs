//! Drawing the overlay: a dimmed screen with a Material side sheet.

use super::{Confirm, Overlay, Section};
use crate::config::OverlayPrefs;
use crate::gamescope;
use crate::sys::quick;
use crate::ui::common;
use crate::ui::steamui::{self, Tab};
use crate::ui::widgets::{self, switch, ButtonKind};
use crate::ui::format_playtime;
use egui::{pos2, vec2, Color32, CornerRadius, Frame, Margin, Rect, RichText, Ui};
use egui_material_icons::icons::*;

const SHEET_WIDTH: f32 = 440.0;

pub fn show(ui: &mut Ui, o: &mut Overlay) {
    if !o.shown {
        return;
    }
    let p = o.palette.clone();
    let screen = ui.ctx().content_rect();
    // Scrim; clicking it closes the overlay.
    let scrim = ui.interact(screen, ui.scope_id().with("scrim"), egui::Sense::click());
    ui.painter().rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(140));
    if scrim.clicked() && o.confirm.is_none() {
        o.set_shown(false);
        return;
    }
    let sheet = Rect::from_min_max(pos2(screen.right() - SHEET_WIDTH, screen.top()), screen.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(sheet));
    Frame::new()
        .fill(p.surface_container_low)
        .corner_radius(CornerRadius { nw: 28, sw: 28, ne: 0, se: 0 })
        .inner_margin(Margin::symmetric(20, 18))
        .show(&mut child, |ui| {
            ui.set_min_size(sheet.size() - vec2(40.0, 36.0));
            header(ui, o);
            ui.add_space(10.0);
            section_tabs(ui, o);
            ui.add_space(6.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match o.section {
                    Section::Quick => quick_section(ui, o),
                    Section::Extras(tab) => extras_section(ui, o, tab),
                }
                ui.add_space(24.0);
            });
        });
    toast(ui, o, sheet);
    confirm_dialog(ui.ctx(), o);
}

fn header(ui: &mut Ui, o: &mut Overlay) {
    let p = o.palette.clone();
    let now = crate::launch::now();
    let game = o.game().cloned();
    ui.horizontal(|ui| {
        match &game {
            Some(g) => {
                let (rect, _) = ui.allocate_exact_size(vec2(56.0, 84.0), egui::Sense::hover());
                crate::ui::art::paint(ui, &mut o.thumbs, g.cover.as_deref(), rect, 12, ICON_SPORTS_ESPORTS);
                ui.vertical(|ui| {
                    ui.label(widgets::title_text(&g.name, 19.0).color(p.on_surface));
                    let session = o.running.as_ref().and_then(|r| r.started).map(|s| ((now - s).max(0) / 60) as u64);
                    let mut line = String::new();
                    if let Some(m) = session {
                        line.push_str(&format!("Playing for {}", if m == 0 { "under a minute".into() } else { format_playtime(Some(m)) }));
                    }
                    if g.playtime_minutes.unwrap_or(0) > 0 {
                        if !line.is_empty() {
                            line.push_str(" · ");
                        }
                        line.push_str(&format!("{} total", format_playtime(g.playtime_minutes)));
                    }
                    ui.label(RichText::new(line).color(p.on_surface_variant));
                });
            }
            None => {
                widgets::icon(ui, ICON_SPORTS_ESPORTS, 30.0, p.primary);
                ui.vertical(|ui| {
                    ui.label(widgets::title_text("Quick settings", 19.0).color(p.on_surface));
                    let name = o.running.as_ref().map(|r| r.name.clone()).unwrap_or_else(|| "No game running".into());
                    ui.label(RichText::new(name).color(p.on_surface_variant));
                });
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            ui.label(widgets::title_text(chrono::Local::now().format("%H:%M").to_string(), 18.0).color(p.on_surface));
        });
    });
}

fn section_tabs(ui: &mut Ui, o: &mut Overlay) {
    let mut options: Vec<(Section, egui_material_icons::MaterialIcon, &str)> = vec![(Section::Quick, ICON_TUNE, "")];
    if let Some(g) = o.game() {
        for t in Tab::for_game(g) {
            options.push((Section::Extras(t), t.icon(), ""));
        }
    }
    if !options.iter().any(|(s, _, _)| *s == o.section) {
        o.section = Section::Quick;
    }
    widgets::segmented(ui, &mut o.section, &options);
}

fn chip_row(ui: &mut Ui, items: &[(u32, &str)], value: u32) -> Option<u32> {
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        for (v, label) in items {
            if widgets::filter_chip(ui, label, *v == value, None).clicked() {
                pick = Some(*v);
            }
        }
    });
    pick
}

fn quick_section(ui: &mut Ui, o: &mut Overlay) {
    let p = o.palette.clone();
    let mut prefs = o.prefs();
    let mut changed = false;

    common::card(ui, "Performance", |ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("Frame rate limit").color(p.on_surface));
        if let Some(v) = chip_row(ui, &[(0, "Off"), (30, "30"), (40, "40"), (60, "60"), (90, "90"), (120, "120")], prefs.fps_limit) {
            prefs.fps_limit = v;
            changed = true;
        }
        ui.add_space(6.0);
        common::row(ui, "Allow tearing", "Lowest latency; may tear.", |ui| {
            changed |= switch(ui, &mut prefs.allow_tearing).changed();
        });
        ui.label(RichText::new("Upscaling").color(p.on_surface));
        let filters: Vec<(u32, &str)> = gamescope::FILTERS.to_vec();
        if let Some(v) = chip_row(ui, &filters, prefs.filter) {
            prefs.filter = v;
            changed = true;
        }
        if matches!(prefs.filter, 2 | 3 | 5) {
            // gamescope: 0 is the sharpest, 20 the softest.
            let mut sharp = 20 - prefs.sharpness.min(20);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sharpness").color(p.on_surface_variant));
                if ui.add(egui::Slider::new(&mut sharp, 0..=20)).changed() {
                    prefs.sharpness = 20 - sharp;
                    changed = true;
                }
            });
        }
        if super::mangohud_available() {
            ui.add_space(6.0);
            ui.label(RichText::new("Performance overlay").color(p.on_surface));
            if let Some(v) = chip_row(ui, &[(0, "Off"), (1, "FPS"), (2, "Bar"), (3, "Full")], prefs.perf_overlay) {
                prefs.perf_overlay = v;
                changed = true;
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let busy = o.shot.running();
            if widgets::button_enabled(ui, ButtonKind::Tonal, Some(ICON_SCREENSHOT_MONITOR), "Screenshot", !busy).clicked() {
                o.screenshot();
            }
            ui.label(RichText::new("or Guide + A").size(12.5).color(p.on_surface_variant));
        });
        ui.add_space(8.0);
    });
    if changed {
        o.set_prefs(prefs);
    }
    if o.running.is_none() {
        common::note(ui, "Settings above are saved per game once a game is running.");
    }

    // Audio.
    if let Some(Some(a)) = o.audio.value.clone() {
        common::card(ui, "Sound", |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let icon = if a.muted { ICON_VOLUME_OFF } else { ICON_VOLUME_UP };
                if widgets::icon_button(ui, icon, if a.muted { "Unmute" } else { "Mute" }, true).clicked() {
                    let _ = quick::set_mute(!a.muted);
                    o.audio = Default::default();
                }
                let mut v = a.volume;
                let r = ui.add(egui::Slider::new(&mut v, 0..=150).suffix("%"));
                if r.changed() {
                    let _ = quick::set_volume(v);
                    if let Some(Some(x)) = o.audio.value.as_mut() {
                        x.volume = v;
                    }
                }
            });
            if a.sinks.len() > 1 {
                let current = a.sinks.iter().find(|(n, _)| *n == a.default_sink).map(|(_, d)| d.clone()).unwrap_or_else(|| a.default_sink.clone());
                egui::ComboBox::from_id_salt("sink").width(ui.available_width() - 8.0).selected_text(current).show_ui(ui, |ui| {
                    for (name, desc) in &a.sinks {
                        if ui.selectable_label(*name == a.default_sink, desc).clicked() {
                            let _ = quick::set_default_sink(name);
                            o.audio = Default::default();
                        }
                    }
                });
            }
            ui.add_space(6.0);
        });
    }

    if let Some(Some(b)) = o.brightness.value {
        common::card(ui, "Brightness", |ui| {
            ui.add_space(6.0);
            let mut v = b;
            ui.horizontal(|ui| {
                widgets::icon(ui, ICON_BRIGHTNESS_MEDIUM, 22.0, p.on_surface_variant);
                if ui.add(egui::Slider::new(&mut v, 1..=100).suffix("%")).drag_stopped() {
                    let _ = quick::set_brightness(v);
                }
            });
            if v != b {
                o.brightness.value = Some(Some(v));
            }
            ui.add_space(6.0);
        });
    }

    if let Some(Some(m)) = o.media.value.clone() {
        common::card(ui, "Now playing", |ui| {
            ui.add_space(6.0);
            ui.label(widgets::title_text(if m.title.is_empty() { &m.player } else { &m.title }, 15.0).color(p.on_surface));
            if !m.artist.is_empty() {
                ui.label(RichText::new(&m.artist).color(p.on_surface_variant));
            }
            ui.horizontal(|ui| {
                if widgets::icon_button(ui, ICON_SKIP_PREVIOUS, "Previous", true).clicked() {
                    let _ = quick::media_command("previous");
                }
                if widgets::icon_button(ui, if m.playing { ICON_PAUSE } else { ICON_PLAY_ARROW }, "Play/pause", true).clicked() {
                    let _ = quick::media_command("play-pause");
                    o.media = Default::default();
                }
                if widgets::icon_button(ui, ICON_SKIP_NEXT, "Next", true).clicked() {
                    let _ = quick::media_command("next");
                    o.media = Default::default();
                }
            });
            ui.add_space(4.0);
        });
    }

    if let Some(bats) = o.batteries.value.clone().filter(|b| !b.is_empty()) {
        common::card(ui, "Controllers", |ui| {
            for b in bats {
                let level = b.percent.map(|p| format!("{p}%")).unwrap_or_else(|| b.level.clone());
                let sub = if b.charging { format!("{level} · charging") } else { level };
                common::list_row(ui, ICON_SPORTS_ESPORTS, b.percent.is_some_and(|p| p > 20), &b.name, &sub, |_| {});
            }
        });
    }

    common::card(ui, "", |ui| {
        ui.add_space(8.0);
        if o.running.is_some() && widgets::button(ui, ButtonKind::Danger, Some(ICON_CLOSE), "Quit game").clicked() {
            o.confirm = Some(Confirm::QuitGame);
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if widgets::button(ui, ButtonKind::Tonal, Some(ICON_LOGOUT), "Exit to desktop").clicked() {
                o.confirm = Some(Confirm::ExitDesktop);
            }
            if widgets::button(ui, ButtonKind::Text, Some(ICON_BEDTIME), "Sleep").clicked() {
                o.confirm = Some(Confirm::Power("suspend"));
            }
            if widgets::button(ui, ButtonKind::Text, Some(ICON_RESTART_ALT), "Restart").clicked() {
                o.confirm = Some(Confirm::Power("reboot"));
            }
            if widgets::button(ui, ButtonKind::Text, Some(ICON_POWER_SETTINGS_NEW), "Shut down").clicked() {
                o.confirm = Some(Confirm::Power("poweroff"));
            }
        });
        ui.add_space(8.0);
    });
    ui.label(RichText::new("Guide: show/hide · Guide + A: screenshot · Guide + B: quit game").size(12.0).color(p.on_surface_variant));
    let _ = OverlayPrefs::default();
}

fn extras_section(ui: &mut Ui, o: &mut Overlay, tab: Tab) {
    let Some(game) = o.game().cloned() else {
        return;
    };
    let extras = steamui::Extras::for_game(&mut o.extras, &game);
    extras.tab = tab;
    let outs = steamui::show(ui, extras, &game, &mut o.config, &mut o.thumbs, true);
    for out in outs {
        match out {
            steamui::Out::SaveConfig => {
                if let Err(e) = o.config.save() {
                    o.notify(format!("Couldn't save: {e}"));
                }
            }
            steamui::Out::OpenUrl(url) => {
                let _ = crate::sys::cmd::spawn_detached("xdg-open", &[&url]);
                o.notify("Opened in your browser");
            }
            steamui::Out::OpenPath(path) => {
                let s = path.to_string_lossy().into_owned();
                let program = if crate::sys::cmd::exists("m3fm") { "m3fm" } else { "xdg-open" };
                let _ = crate::sys::cmd::spawn_detached(program, &[&s]);
            }
        }
    }
}

fn toast(ui: &mut Ui, o: &Overlay, sheet: Rect) {
    let Some((text, _)) = &o.toast else {
        return;
    };
    let p = &o.palette;
    let at = pos2(sheet.left() - 16.0, sheet.bottom() - 24.0);
    egui::Area::new(egui::Id::unique("overlay-toast"))
        .fixed_pos(at)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            Frame::new().fill(p.inverse_surface).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(16, 10)).show(ui, |ui| {
                ui.set_max_width(420.0);
                ui.add(egui::Label::new(RichText::new(text).color(p.inverse_on_surface)).wrap());
            });
        });
}

fn confirm_dialog(ctx: &egui::Context, o: &mut Overlay) {
    let Some(c) = o.confirm else {
        return;
    };
    let p = o.palette.clone();
    let game = o.running.as_ref().map(|r| r.name.clone()).unwrap_or_default();
    let (icon, title, body, button) = match c {
        Confirm::QuitGame => (ICON_CLOSE, format!("Quit {game}?"), "Unsaved progress in the game may be lost.".to_string(), "Quit game"),
        Confirm::ExitDesktop => (ICON_LOGOUT, "Exit to desktop?".into(), if o.running.is_some() { "The game keeps running until gamescope closes, which ends it.".into() } else { "Back to Hyprland.".into() }, "Exit"),
        Confirm::Power("suspend") => (ICON_BEDTIME, "Sleep?".into(), String::new(), "Sleep"),
        Confirm::Power("reboot") => (ICON_RESTART_ALT, "Restart?".into(), "Running games are closed.".into(), "Restart"),
        Confirm::Power(_) => (ICON_POWER_SETTINGS_NEW, "Shut down?".into(), "Running games are closed.".into(), "Shut down"),
    };
    let mut close = false;
    let mut go = false;
    let r = common::modal(ctx, &p, "overlay-confirm", icon, &title, |ui| {
        ui.set_max_width(380.0);
        if !body.is_empty() {
            ui.label(RichText::new(&body).color(p.on_surface_variant));
        }
        common::dialog_buttons(ui, |ui| {
            let b = widgets::button(ui, ButtonKind::Danger, None, button);
            if b.clicked() {
                go = true;
            }
            // Controller users land on the confirm button.
            if !ui.ctx().memory(|m| m.focused().is_some()) {
                b.request_focus();
            }
            if widgets::button(ui, ButtonKind::Text, None, "Cancel").clicked() {
                close = true;
            }
        });
    });
    if go {
        o.confirm = None;
        match c {
            Confirm::QuitGame => o.quit_game(),
            Confirm::ExitDesktop => {
                if let Err(e) = crate::session::exit() {
                    o.notify(e);
                }
            }
            Confirm::Power(a) => {
                if let Err(e) = quick::power(a) {
                    o.notify(e);
                }
            }
        }
    } else if close || r.should_close() {
        o.confirm = None;
    }
}
