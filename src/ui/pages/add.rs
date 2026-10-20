//! Add a game by hand (or edit one): name, executable, runner for Windows
//! games, arguments, working folder, environment and cover.

use crate::app::App;
use crate::library::manual::{self, ManualGame, Runner};
use crate::sys::{cmd, task::Task};
use crate::ui::widgets::{self, segmented, ButtonKind};
use crate::ui::{common, Action, Page, ToastKind};
use egui::Ui;
use egui_material_icons::icons::*;

#[derive(Default)]
pub struct State {
    /// Which game the form holds: None = not loaded, Some(None) = new.
    loaded: Option<Option<String>>,
    game: ManualGame,
    env: String,
    browse: Task<Option<(Field, String)>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Exe,
    Dir,
    Cover,
}

/// A native file dialog through zenity or kdialog, if one is installed.
fn pick(directory: bool, images: bool) -> Option<String> {
    let out = if cmd::exists("zenity") {
        let mut args = vec!["--file-selection"];
        if directory {
            args.push("--directory");
        }
        if images {
            args.push("--file-filter=Images | *.png *.jpg *.jpeg *.webp");
        }
        cmd::run("zenity", &args).ok()?
    } else if cmd::exists("kdialog") {
        let home = crate::sys::paths::home().to_string_lossy().into_owned();
        if directory {
            cmd::run("kdialog", &["--getexistingdirectory", &home]).ok()?
        } else {
            cmd::run("kdialog", &["--getopenfilename", &home]).ok()?
        }
    } else {
        return None;
    };
    Some(out.trim().to_string()).filter(|s| !s.is_empty())
}

fn path_row(ui: &mut Ui, state: &mut State, ctx: &egui::Context, field: Field, title: &str, desc: &str, hint: &str) {
    let can_browse = cmd::exists("zenity") || cmd::exists("kdialog");
    common::row(ui, title, desc, |ui| {
        if can_browse && widgets::icon_button(ui, ICON_FOLDER_OPEN, "Browse", !state.browse.running()).clicked() {
            state.browse.spawn(ctx, move || pick(field == Field::Dir, field == Field::Cover).map(|p| (field, p)));
        }
        let value = match field {
            Field::Exe => &mut state.game.exe,
            Field::Dir => &mut state.game.dir,
            Field::Cover => &mut state.game.cover,
        };
        ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(300.0));
    });
}

pub fn show(ui: &mut Ui, app: &mut App, slug: Option<&str>) {
    let mut actions = Vec::new();
    let ctx = app.ctx.clone();
    let state = &mut app.pages.add;
    if state.loaded.as_ref().map(|s| s.as_deref()) != Some(slug) {
        state.game = slug
            .and_then(|s| manual::load().into_iter().find(|g| g.slug == s))
            .unwrap_or_else(|| ManualGame { proton: "GE-Proton".into(), ..Default::default() });
        state.env = state.game.env.join("\n");
        state.loaded = Some(slug.map(str::to_string));
    }
    if let Some(Some((field, path))) = state.browse.take() {
        match field {
            Field::Exe => {
                if state.game.name.is_empty() {
                    let stem = std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    state.game.name = stem;
                }
                if path.to_lowercase().ends_with(".exe") && state.game.runner == Runner::Native {
                    state.game.runner = Runner::Umu;
                }
                state.game.exe = path;
            }
            Field::Dir => state.game.dir = path,
            Field::Cover => state.game.cover = path,
        }
    }
    let editing = slug.is_some();
    common::page(
        ui,
        ICON_ADD_CIRCLE,
        if editing { "Edit game" } else { "Add a game" },
        "For games outside Steam, Lutris and Heroic: native binaries, or Windows .exe files run with Proton through umu-launcher.",
        |ui| {
            common::card(ui, "Game", |ui| {
                common::row(ui, "Name", "", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut state.game.name).hint_text("My Game").desired_width(300.0));
                });
                ui.separator();
                path_row(ui, state, &ctx, Field::Exe, "Executable", "The program or .exe to run.", "/path/to/game or game.exe");
                ui.separator();
                common::row(ui, "Runs with", "Proton (umu) is best for most Windows games; each game gets its own prefix.", |ui| {
                    segmented(ui, &mut state.game.runner, &[(Runner::Native, ICON_TERMINAL, "Native"), (Runner::Umu, ICON_WINE_BAR, "Proton"), (Runner::Wine, ICON_LIQUOR, "Wine")]);
                });
                if state.game.runner == Runner::Umu {
                    common::require(ui, &mut actions, "umu-run", "umu-launcher", "It downloads and runs Proton for you.");
                    ui.separator();
                    common::row(ui, "Proton version", "GE-Proton = the latest GE-Proton, downloaded by umu. Or a path to a Proton folder.", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut state.game.proton).desired_width(300.0));
                    });
                }
                if state.game.runner == Runner::Wine {
                    common::require(ui, &mut actions, "wine", "wine", "");
                }
                ui.separator();
                common::row(ui, "Arguments", "Passed to the game.", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut state.game.args).hint_text("-windowed").desired_width(300.0));
                });
                ui.separator();
                path_row(ui, state, &ctx, Field::Dir, "Working folder", "Defaults to the executable's folder.", "optional");
                ui.separator();
                path_row(ui, state, &ctx, Field::Cover, "Cover image", "Portrait art looks best (600×900).", "optional");
                ui.separator();
                common::row(ui, "Environment", "One KEY=value per line.", |_| {});
                ui.add(egui::TextEdit::multiline(&mut state.env).code_editor().desired_rows(3).desired_width(f32::INFINITY).hint_text("DXVK_ASYNC=1"));
                ui.add_space(8.0);
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                let valid = !state.game.name.trim().is_empty() && !state.game.exe.trim().is_empty();
                if widgets::button_enabled(ui, ButtonKind::Filled, Some(ICON_SAVE), if editing { "Save" } else { "Add game" }, valid).clicked() {
                    let mut g = state.game.clone();
                    g.name = g.name.trim().to_string();
                    g.exe = g.exe.trim().to_string();
                    g.env = state.env.lines().map(str::trim).filter(|l| l.contains('=')).map(str::to_string).collect();
                    if !editing {
                        g.slug = crate::library::slugify(&g.name);
                    }
                    match manual::upsert(g, slug) {
                        Ok(new_slug) => {
                            state.loaded = None;
                            actions.push(Action::Rescan);
                            actions.push(Action::Toast("Saved".into(), ToastKind::Success));
                            actions.push(Action::Goto(Page::Detail(format!("manual:{new_slug}"))));
                        }
                        Err(e) => actions.push(Action::Toast(format!("Couldn't save: {e}"), ToastKind::Error)),
                    }
                }
                if widgets::button(ui, ButtonKind::Text, None, "Cancel").clicked() {
                    state.loaded = None;
                    actions.push(Action::Back);
                }
            });
        },
    );
    app.actions.extend(actions);
}
