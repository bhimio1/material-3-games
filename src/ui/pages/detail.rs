//! One game: hero art, Play, stats, and per-game launch options.

use crate::app::App;
use crate::library::{Launch, Source};
use crate::ui::widgets::{self, switch, ButtonKind};
use crate::ui::{art, common, format_last_played, format_playtime, steamui, Action, Page, SteamJob};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, Mesh, Rect, RichText, Ui};
use material_3_ui::icons::icons::*;

#[derive(Default)]
pub struct State {
    /// (game id, env text being edited)
    env: Option<(String, String)>,
    confirm_remove: bool,
    pub extras: Option<steamui::Extras>,
    steam: Option<SteamCfg>,
}

/// What Steam has stored for a Steam game, read when the page opens.
struct SteamCfg {
    game_id: String,
    compat: String,
    launch: String,
    tools: Vec<crate::steam::appconfig::CompatTool>,
}

impl SteamCfg {
    fn load(game_id: &str, appid: &str) -> SteamCfg {
        use crate::steam::{account, appconfig};
        let root = account::root(&crate::sys::paths::home());
        let compat = root
            .as_deref()
            .and_then(|r| std::fs::read_to_string(appconfig::config_vdf(r)).ok())
            .and_then(|t| appconfig::get_compat(&t, appid))
            .unwrap_or_default();
        let launch = root
            .as_deref()
            .and_then(account::user_dir)
            .and_then(|u| std::fs::read_to_string(appconfig::localconfig(&u)).ok())
            .map(|t| appconfig::get_launch_options(&t, appid))
            .unwrap_or_default();
        SteamCfg {
            game_id: game_id.into(),
            compat,
            launch,
            tools: root.as_deref().map(appconfig::list_tools).unwrap_or_default(),
        }
    }
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

            steam_card(ui, app, &game, &mut opts, &mut changed, &mut actions);

            // Achievements, guides, news, notes, screenshots.
            common::card(ui, "", |ui| {
                ui.add_space(8.0);
                let extras = steamui::Extras::for_game(&mut app.pages.detail.extras, &game);
                steamui::tabs(ui, extras, &game);
                ui.add_space(4.0);
                let outs = steamui::show(ui, extras, &game, &mut app.config, &mut app.thumbs, false);
                for o in outs {
                    actions.push(match o {
                        steamui::Out::SaveConfig => Action::ConfigChanged,
                        steamui::Out::OpenUrl(u) => Action::OpenUrl(u),
                        steamui::Out::OpenPath(p) => Action::OpenPath(p),
                    });
                }
                // Notes and pins edit the config directly.
                opts = app.config.options(&game.id);
                ui.add_space(8.0);
            });

            let store = matches!(game.launch, Launch::Steam { .. } | Launch::Lutris { .. } | Launch::Heroic { .. });
            common::card(ui, "Launch options", |ui| {
                if store {
                    let hint = match game.source {
                        Source::Steam => "Steam starts this game, so GameMode, MangoHud and gamescope go in its Steam launch options instead (e.g. gamemoderun mangohud %command%, or game-performance %command% on CachyOS).",
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
                    if opts.cachyos_perf || crate::sys::cmd::exists(crate::steam::appconfig::GAME_PERFORMANCE) {
                        common::row(ui, "CachyOS performance profile", "Run through CachyOS's game-performance, which switches to the performance power profile while playing.", |ui| {
                            changed |= switch(ui, &mut opts.cachyos_perf).changed();
                        });
                        if opts.cachyos_perf && opts.gamemode {
                            common::note(ui, "GameMode and the CachyOS profile both tune the CPU; one of them is usually enough.");
                        }
                        if opts.cachyos_perf {
                            common::require(ui, &mut actions, crate::steam::appconfig::GAME_PERFORMANCE, "cachyos-settings", "");
                        }
                        ui.separator();
                    }
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

/// Steam: Steam Input for any game, plus Proton / launch options / store
/// actions for Steam games.
fn steam_card(ui: &mut Ui, app: &mut App, game: &crate::library::Game, opts: &mut crate::config::GameOptions, changed: &mut bool, actions: &mut Vec<Action>) {
    let steam_installed = crate::sys::cmd::exists("steam");
    let Some(appid) = game.steam_appid().map(str::to_string) else {
        // Non-Steam game: Steam Input through a shortcut.
        common::card(ui, "Steam Input", |ui| {
            common::row(
                ui,
                "Use Steam Input",
                "Adds this game to Steam as a non-Steam shortcut and launches it through Steam, so your controller layouts, gyro and Steam's overlay work. Your options here still apply.",
                |ui| {
                    let mut on = opts.steam_input && game.shortcut_appid.is_some();
                    if switch(ui, &mut on).changed() {
                        opts.steam_input = on;
                        *changed = true;
                        let job = if on {
                            SteamJob::Shortcuts { add: vec![game.id.clone()], remove: vec![] }
                        } else {
                            SteamJob::Shortcuts { add: vec![], remove: vec![game.id.clone()] }
                        };
                        actions.push(Action::Steam(job));
                    }
                },
            );
            if !steam_installed {
                common::require(ui, actions, "steam", "steam", "Steam Input needs Steam.");
            }
            if let Some(sc) = game.shortcut_appid {
                ui.separator();
                common::row(ui, "Controller layout", "Open Steam's controller configurator for this shortcut.", |ui| {
                    if widgets::button(ui, ButtonKind::Tonal, Some(ICON_SPORTS_ESPORTS), "Configure").clicked() {
                        let _ = crate::steam::open(&crate::steam::url("controller", &crate::steam::shortcuts::rungameid(sc).to_string()));
                    }
                });
            }
        });
        return;
    };

    let state = &mut app.pages.detail;
    if state.steam.as_ref().is_none_or(|s| s.game_id != game.id) {
        state.steam = Some(SteamCfg::load(&game.id, &appid));
    }
    let cfg = state.steam.as_mut().expect("loaded above");
    common::card(ui, "Steam", |ui| {
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let mut go = |ui: &mut Ui, icon, label: &str, action: &str| {
                if widgets::button(ui, ButtonKind::Tonal, Some(icon), label).clicked() {
                    if let Err(e) = crate::steam::open(&crate::steam::url(action, &appid)) {
                        actions.push(Action::Toast(e, crate::ui::ToastKind::Error));
                    }
                }
            };
            if game.installed {
                go(ui, ICON_SPORTS_ESPORTS, "Controller layout", "controller");
                go(ui, ICON_VERIFIED, "Verify files", "validate");
                go(ui, ICON_TUNE, "Properties", "properties");
            } else {
                go(ui, ICON_DOWNLOAD, "Install", "install");
            }
            go(ui, ICON_STOREFRONT, "Store", "store");
            go(ui, ICON_FORUM, "Community", "hub");
            if game.installed {
                go(ui, ICON_DELETE, "Uninstall", "uninstall");
            }
        });
        ui.add_space(8.0);
        ui.separator();
        // Compatibility tool.
        common::row(ui, "Compatibility tool", "Proton version for this game. Saving restarts Steam if it's running.", |ui| {
            let current = if cfg.compat.is_empty() { "Steam default".to_string() } else { cfg.tools.iter().find(|t| t.name == cfg.compat).map(|t| t.display.clone()).unwrap_or(cfg.compat.clone()) };
            egui::ComboBox::from_id_salt(("compat", &game.id)).width(220.0).selected_text(current).show_ui(ui, |ui| {
                let mut pick: Option<String> = None;
                if ui.selectable_label(cfg.compat.is_empty(), "Steam default").clicked() {
                    pick = Some(String::new());
                }
                for t in &cfg.tools {
                    if ui.selectable_label(cfg.compat == t.name, &t.display).clicked() {
                        pick = Some(t.name.clone());
                    }
                }
                if let Some(tool) = pick.filter(|t| *t != cfg.compat) {
                    cfg.compat = tool.clone();
                    actions.push(Action::Steam(SteamJob::Compat { appid: appid.clone(), tool }));
                }
            });
        });
        ui.separator();
        // Launch options with helper toggles.
        common::row(ui, "Launch options", "Steam's launch options for this game. Helpers add the common ones around %command%.", |_| {});
        ui.horizontal_wrapped(|ui| {
            for (label, t) in crate::steam::appconfig::available_toggles() {
                let on = crate::steam::appconfig::has_toggle(&cfg.launch, *t);
                if widgets::filter_chip(ui, label, on, None).clicked() {
                    cfg.launch = crate::steam::appconfig::set_toggle(&cfg.launch, *t, !on);
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut cfg.launch).hint_text("%command%").desired_width(ui.available_width() - 110.0).font(egui::TextStyle::Monospace));
            if widgets::button(ui, ButtonKind::Filled, Some(ICON_SAVE), "Save").clicked() {
                actions.push(Action::Steam(SteamJob::LaunchOptions { appid: appid.clone(), options: cfg.launch.clone() }));
            }
        });
        ui.add_space(8.0);
    });
    let _ = changed;
}
