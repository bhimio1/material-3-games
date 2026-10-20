//! Desktop-window pages.

pub mod add;
pub mod detail;
pub mod gamemode;
pub mod library;
pub mod settings;

use super::Page;
use crate::app::App;

#[derive(Default)]
pub struct States {
    pub add: add::State,
    pub detail: detail::State,
    pub settings: settings::State,
}

pub fn show(ui: &mut egui::Ui, app: &mut App) {
    match app.page.clone() {
        Page::Library(f) => library::show(ui, app, f),
        Page::Detail(id) => detail::show(ui, app, &id),
        Page::Add(slug) => add::show(ui, app, slug.as_deref()),
        Page::GameMode => gamemode::show(ui, app),
        Page::Settings => settings::show(ui, app),
    }
}
