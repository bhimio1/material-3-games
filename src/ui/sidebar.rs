//! Navigation drawer: library filters with counts, Game Mode, Settings.

use super::widgets::{self, nav_item, section_header};
use super::{Action, Filter, Page};
use crate::app::App;
use crate::library::Source;
use egui::{vec2, CornerRadius, Sense, Ui};
use material_3_ui::icons::icons::*;

pub fn show(ui: &mut Ui, app: &mut App) {
    let p = app.palette.clone();
    let mut actions = Vec::new();
    let current = app.page.clone();

    ui.horizontal(|ui| {
        ui.add_space(12.0);
        widgets::icon(ui, ICON_SPORTS_ESPORTS, 26.0, p.primary);
        ui.label(widgets::title_text("Games", 20.0).color(p.on_surface));
    });
    ui.add_space(14.0);

    // Extended FAB: Game Mode.
    let size = vec2(ui.available_width().min(190.0), 52.0);
    let (rect, fab) = ui.allocate_exact_size(size, Sense::click());
    let fill = widgets::state_layer(p.primary_container, p.on_primary_container, &fab);
    ui.painter().rect_filled(rect, CornerRadius::same(16), fill);
    widgets::paint_icon(ui, ICON_VIDEOGAME_ASSET, egui::pos2(rect.left() + 28.0, rect.center().y), 24.0, p.on_primary_container);
    ui.painter().text(
        egui::pos2(rect.left() + 52.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Game Mode",
        widgets::title_font(15.0),
        p.on_primary_container,
    );
    if fab.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Big-picture mode in gamescope").clicked() {
        actions.push(Action::Goto(Page::GameMode));
    }
    ui.add_space(14.0);

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let count = |f: Filter| app.games.iter().filter(|g| f.matches(g)).count();
        section_header(ui, "Library");
        let items = [
            (Filter::All, ICON_APPS),
            (Filter::Recent, ICON_HISTORY),
            (Filter::Installed, ICON_DOWNLOAD_DONE),
            (Filter::Source(Source::Steam), ICON_CLOUD),
            (Filter::Source(Source::Lutris), ICON_WINE_BAR),
            (Filter::Source(Source::Heroic), ICON_SHIELD),
            (Filter::Other, ICON_EXTENSION),
        ];
        for (f, icon) in items {
            let n = count(f);
            if n == 0 && !matches!(f, Filter::All) {
                continue;
            }
            let selected = current == Page::Library(f);
            if nav_item(ui, icon, f.title(), selected, Some(&n.to_string())).clicked() {
                actions.push(Action::Goto(Page::Library(f)));
            }
        }
        if nav_item(ui, ICON_ADD_CIRCLE, "Add a game", matches!(current, Page::Add(_)), None).clicked() {
            actions.push(Action::Goto(Page::Add(None)));
        }
        section_header(ui, "Play");
        if nav_item(ui, ICON_VIDEOGAME_ASSET, "Game Mode", current == Page::GameMode, None).clicked() {
            actions.push(Action::Goto(Page::GameMode));
        }
        if nav_item(ui, ICON_SETTINGS, "Settings", current == Page::Settings, None).clicked() {
            actions.push(Action::Goto(Page::Settings));
        }
        if app.scanning() {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.add(egui::Spinner::new().color(p.primary).size(14.0));
                ui.label(egui::RichText::new("Scanning your games…").size(12.5).color(p.on_surface_variant));
            });
        }
    });
    app.actions.extend(actions);
}
