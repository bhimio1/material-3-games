//! Settings: game sources, covers, Hyprland game rules, gamescope defaults,
//! appearance.

use crate::app::App;
use crate::config::{ColorScheme, ThemeSource};
use crate::hypr;
use crate::sys::{cmd, paths, term};
use crate::theme::palette::{parse_hex, to_hex};
use crate::ui::common::{self, card, row};
use crate::ui::widgets::{self, segmented, switch, ButtonKind};
use crate::ui::Action;
use egui::{vec2, Color32, Ui};
use egui_material_icons::icons::*;

const SEED_PRESETS: &[&str] = &[
    "#7C4DFF", "#4285F4", "#6750A4", "#B3261E", "#E8710A", "#1E8E3E", "#00897B", "#C2185B",
    "#607D8B",
];

#[derive(Default)]
pub struct State {
    seed_text: Option<String>,
}

pub fn show(ui: &mut Ui, app: &mut App) {
    let p = app.palette.clone();
    let theme_file = app.theme_file();
    let rules_installed = app.rules_installed;
    let fetching = app.fetching_covers();
    let counts: Vec<(crate::library::Source, usize)> = [
        crate::library::Source::Steam,
        crate::library::Source::Lutris,
        crate::library::Source::Heroic,
        crate::library::Source::Desktop,
    ]
    .into_iter()
    .map(|s| (s, app.games.iter().filter(|g| g.source == s).count()))
    .collect();
    let count = |s| counts.iter().find(|(x, _)| *x == s).map(|(_, n)| *n).unwrap_or(0);
    let config = &mut app.config;
    let st = &mut app.pages.settings;
    let mut actions = Vec::new();
    let mut changed = false;
    let mut rescan = false;
    let home = paths::home();
    let target = hypr::target(&home);

    common::page(ui, ICON_SETTINGS, "Settings", "Saved to ~/.config/m3-games/config.toml.", |ui| {
        card(ui, "Game sources", |ui| {
            use crate::library::Source;
            let sources: [(&str, &str, &mut bool, Source); 4] = [
                ("Steam", "Installed Steam games, with art from Steam's library cache.", &mut config.steam, Source::Steam),
                ("Lutris", "Games from your Lutris library.", &mut config.lutris, Source::Lutris),
                ("Heroic", "Installed Epic, GOG and Amazon games from Heroic.", &mut config.heroic, Source::Heroic),
                ("Apps", "Native games from your application menu.", &mut config.desktop_entries, Source::Desktop),
            ];
            let n = sources.len();
            for (i, (title, desc, flag, src)) in sources.into_iter().enumerate() {
                let desc = format!("{desc} {} found.", count(src));
                row(ui, title, &desc, |ui| {
                    if switch(ui, flag).changed() {
                        changed = true;
                        rescan = true;
                    }
                });
                if i + 1 < n {
                    ui.separator();
                }
            }
        });

        card(ui, "Cover art", |ui| {
            row(ui, "SteamGridDB API key", "Finds covers for games that have none. Get a free key at steamgriddb.com → Preferences → API.", |ui| {
                if widgets::icon_button(ui, ICON_OPEN_IN_NEW, "Get a key", true).clicked() {
                    actions.push(Action::OpenUrl("https://www.steamgriddb.com/profile/preferences/api".into()));
                }
                let r = ui.add(egui::TextEdit::singleline(&mut config.steamgriddb_key).password(true).desired_width(240.0).hint_text("optional"));
                if r.lost_focus() {
                    changed = true;
                }
            });
            ui.separator();
            row(ui, "Missing covers", "Download art for games without a cover.", |ui| {
                if fetching {
                    ui.add(egui::Spinner::new().color(p.primary));
                } else if widgets::button(ui, ButtonKind::Tonal, Some(ICON_IMAGE), "Fetch now").clicked() {
                    actions.push(Action::FetchCovers);
                }
            });
        });

        card(ui, "Hyprland", |ui| {
            let syntax = match target.syntax {
                hypr::Syntax::Lua => "Lua config",
                hypr::Syntax::Conf => "hyprland.conf (0.53+ rule syntax)",
            };
            let desc = format!(
                "Allows tearing and turns off idle, animations, blur and shadows for Steam games, gamescope, .exe windows and games launched here. Written to {} ({syntax}).",
                paths::tilde(&target.path)
            );
            row(ui, "Game window rules", &desc, |ui| {
                if rules_installed {
                    if widgets::button(ui, ButtonKind::Outlined, Some(ICON_DELETE), "Remove").clicked() {
                        actions.push(Action::InstallRules(false));
                    }
                    widgets::icon(ui, ICON_CHECK_CIRCLE, 22.0, p.primary);
                } else if widgets::button(ui, ButtonKind::Filled, Some(ICON_ADD), "Install").clicked() {
                    actions.push(Action::InstallRules(true));
                }
            });
            ui.separator();
            row(ui, "Performance mode while playing", "Turn off animations, blur and shadows while a game launched from here is running.", |ui| {
                changed |= switch(ui, &mut config.performance_mode).changed();
            });
            if !config.learned_classes.is_empty() {
                ui.separator();
                let list = config.learned_classes.join(", ");
                row(ui, "Learned game windows", &list, |ui| {
                    if widgets::button(ui, ButtonKind::Text, None, "Forget").clicked() {
                        config.learned_classes.clear();
                        changed = true;
                    }
                });
            }
        });

        card(ui, "gamescope", |ui| {
            common::note(ui, "Used by Game Mode, and by games set to run in gamescope. 0 means your monitor's own value.");
            let g = &mut config.gamescope;
            row(ui, "Resolution", "Output size.", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut g.height).range(0..=4320).suffix(" px")).changed();
                ui.label("×");
                changed |= ui.add(egui::DragValue::new(&mut g.width).range(0..=7680).suffix(" px")).changed();
            });
            ui.separator();
            row(ui, "Refresh rate", "", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut g.refresh).range(0..=500).suffix(" Hz")).changed();
            });
            ui.separator();
            row(ui, "Adaptive sync", "VRR / FreeSync when the monitor supports it.", |ui| {
                changed |= switch(ui, &mut g.adaptive_sync).changed();
            });
            ui.separator();
            row(ui, "FSR upscaling", "AMD FidelityFX Super Resolution when a game renders below the output size.", |ui| {
                changed |= switch(ui, &mut g.fsr).changed();
            });
            ui.separator();
            row(ui, "HDR", "Needs an HDR monitor and gamescope's DRM backend (full Game Mode).", |ui| {
                changed |= switch(ui, &mut g.hdr).changed();
            });
            ui.separator();
            row(ui, "Extra arguments", "Added to every gamescope command, e.g. --mangoapp.", |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut g.extra).desired_width(240.0));
                if r.lost_focus() {
                    changed = true;
                }
            });
        });

        card(ui, "Appearance", |ui| {
            let source_desc = match (&config.theme_source, &theme_file) {
                (ThemeSource::Auto, Some(f)) => format!("Following {} (updates live with your wallpaper).", paths::tilde(f)),
                (ThemeSource::Auto, None) => "No end-4 or file manager theme found, so the seed color below is used.".to_string(),
                (ThemeSource::Seed, _) => "Using a palette generated from the seed color below.".to_string(),
            };
            row(ui, "Color source", &source_desc, |ui| {
                changed |= segmented(ui, &mut config.theme_source, &[
                    (ThemeSource::Auto, ICON_WALLPAPER, "Auto"),
                    (ThemeSource::Seed, ICON_PALETTE, "Seed"),
                ]);
            });
            ui.separator();
            let seed_mode = theme_file.is_none();
            ui.add_enabled_ui(seed_mode, |ui| {
                row(ui, "Color scheme", if seed_mode { "Light, dark, or follow the system." } else { "The theme file decides light or dark." }, |ui| {
                    changed |= segmented(ui, &mut config.color_scheme, &[
                        (ColorScheme::System, ICON_BRIGHTNESS_AUTO, "System"),
                        (ColorScheme::Light, ICON_LIGHT_MODE, "Light"),
                        (ColorScheme::Dark, ICON_DARK_MODE, "Dark"),
                    ]);
                });
                ui.separator();
                row(ui, "Seed color", "Every color in the interface is derived from this one.", |ui| {
                    let text = st.seed_text.get_or_insert_with(|| config.seed_color.clone());
                    let resp = ui.add(egui::TextEdit::singleline(text).desired_width(92.0).char_limit(7));
                    if resp.changed() {
                        if let Some(c) = parse_hex(text) {
                            config.seed_color = to_hex(c);
                            changed = true;
                        }
                    }
                    let mut color = parse_hex(&config.seed_color).unwrap_or(Color32::from_rgb(0x42, 0x85, 0xF4));
                    if ui.color_edit_button_srgba(&mut color).changed() {
                        config.seed_color = to_hex(color);
                        *text = config.seed_color.clone();
                        changed = true;
                    }
                    for hex in SEED_PRESETS.iter().rev() {
                        let c = parse_hex(hex).expect("valid preset");
                        let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::click());
                        ui.painter().circle_filled(rect.center(), 11.0, c);
                        if config.seed_color.eq_ignore_ascii_case(hex) {
                            ui.painter().circle_stroke(rect.center(), 12.5, egui::Stroke::new(2.0, p.on_surface));
                        }
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            config.seed_color = hex.to_string();
                            *text = hex.to_string();
                            changed = true;
                        }
                    }
                });
            });
            ui.separator();
            row(ui, "Filled icons", "Use filled Material Symbols instead of outlined ones.", |ui| {
                changed |= switch(ui, &mut config.filled_icons).changed();
            });
            ui.separator();
            row(ui, "Interface scale", "Size of everything in the window.", |ui| {
                let r = ui.add(egui::Slider::new(&mut config.ui_scale, 0.75..=1.75).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                // Apply on release so the slider doesn't jump under the cursor.
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    changed = true;
                }
            });
        });

        card(ui, "Behaviour", |ui| {
            let detected = term::pick("").unwrap_or_else(|| "none found".into());
            row(ui, "Terminal", &format!("For installing missing tools. Automatic: {detected}."), |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut config.terminal).hint_text("Automatic").desired_width(180.0));
                if r.lost_focus() {
                    changed = true;
                }
            });
            ui.separator();
            let path = crate::config::Config::path();
            row(ui, "Configuration file", &paths::tilde(&path), |ui| {
                if widgets::button(ui, ButtonKind::Text, Some(ICON_FOLDER_OPEN), "Show").clicked() {
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                        actions.push(Action::OpenPath(dir.to_path_buf()));
                    }
                }
            });
        });

        ui.add_space(24.0);
        let tools = ["gamescope", "gamemoderun", "mangohud", "umu-run", "lutris", "steam"]
            .iter()
            .map(|t| format!("{t} {}", if cmd::exists(t) { "✓" } else { "–" }))
            .collect::<Vec<_>>()
            .join("   ");
        ui.label(egui::RichText::new(format!("Material 3 Games {}   ·   {tools}", env!("CARGO_PKG_VERSION"))).size(12.0).color(p.on_surface_variant));
    });

    if rescan {
        actions.push(Action::Rescan);
    }
    if changed {
        actions.push(Action::ConfigChanged);
    }
    app.actions.extend(actions);
}
