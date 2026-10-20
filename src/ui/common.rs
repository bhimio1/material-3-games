//! Page scaffolding shared by every settings page: the scrolling page body,
//! titled cards, and title/description/control rows.

use super::widgets::{self, ButtonKind};
use super::{Action, Actions};
use crate::theme::M3Palette;
use egui::{vec2, Align2, Color32, CornerRadius, Frame, Id, Margin, RichText, Sense, Ui};
use egui_material_icons::{icons::*, MaterialIcon};

/// Widest the page content gets.
pub const PAGE_WIDTH: f32 = 880.0;

/// A scrolling page with a large header.
pub fn page(ui: &mut Ui, icon: MaterialIcon, title: &str, subtitle: &str, body: impl FnOnce(&mut Ui)) {
    let p = widgets::palette(ui);
    egui::ScrollArea::vertical()
        .id_salt(("page", title))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            Frame::new().inner_margin(Margin::symmetric(32, 24)).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(PAGE_WIDTH));
                ui.horizontal(|ui| {
                    widgets::icon(ui, icon, 30.0, p.primary);
                    ui.label(RichText::new(title).size(28.0).color(p.on_surface));
                });
                if !subtitle.is_empty() {
                    ui.add(egui::Label::new(RichText::new(subtitle).color(p.on_surface_variant)).wrap());
                }
                body(ui);
                ui.add_space(24.0);
            });
        });
}

/// A titled group of rows.
pub fn card(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let p = widgets::palette(ui);
    ui.add_space(20.0);
    if !title.is_empty() {
        ui.label(widgets::title_text(title, 14.0).color(p.primary));
        ui.add_space(6.0);
    }
    Frame::new()
        .fill(p.surface_container)
        .corner_radius(CornerRadius::same(20))
        .inner_margin(Margin::symmetric(20, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// A settings row: title and description on the left, control on the right.
pub fn row(ui: &mut Ui, title: &str, desc: &str, control: impl FnOnce(&mut Ui)) {
    let p = widgets::palette(ui);
    ui.add_space(8.0);
    // Controls are laid out first (from the right) so the text wraps in
    // whatever width they leave.
    let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
        control(ui);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.vertical(|ui| {
                ui.set_min_width(ui.available_width().min(160.0));
                ui.label(RichText::new(title).size(14.5).color(p.on_surface));
                if !desc.is_empty() {
                    ui.add(egui::Label::new(RichText::new(desc).size(12.5).color(p.on_surface_variant)).wrap());
                }
            });
        });
    });
    ui.add_space(8.0);
}

/// Body text inside a card.
pub fn note(ui: &mut Ui, text: &str) {
    let p = widgets::palette(ui);
    ui.add_space(8.0);
    ui.add(egui::Label::new(RichText::new(text).size(13.0).color(p.on_surface_variant)).wrap());
    ui.add_space(8.0);
}

/// A tinted banner for warnings and status messages.
pub fn banner(ui: &mut Ui, icon: MaterialIcon, text: &str, error: bool, actions: impl FnOnce(&mut Ui)) {
    let p = widgets::palette(ui);
    let (bg, fg) = if error {
        (p.error_container, p.on_error_container)
    } else {
        (p.tertiary_container, p.on_tertiary_container)
    };
    ui.add_space(12.0);
    Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                actions(ui);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    widgets::icon(ui, icon, 20.0, fg);
                    ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                });
            });
        });
}

/// A spinner with a label, for values still loading.
pub fn loading(ui: &mut Ui, text: &str) {
    let p = widgets::palette(ui);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().color(p.primary));
        ui.label(RichText::new(text).color(p.on_surface_variant));
    });
    ui.add_space(8.0);
}

/// Check that a command-line tool is available. When it isn't, show a
/// banner offering to install its package and return false.
pub fn require(ui: &mut Ui, actions: &mut Actions, program: &str, package: &str, purpose: &str) -> bool {
    if crate::sys::cmd::exists(program) {
        return true;
    }
    let text = format!("`{program}` isn't installed ({package}). {purpose}");
    banner(ui, ICON_WARNING, &text, false, |ui| {
        if widgets::button(ui, ButtonKind::Filled, Some(ICON_DOWNLOAD), "Install").clicked() {
            actions.push(Action::Install {
                packages: vec![package.to_string()],
                aur: false,
            });
        }
    });
    false
}

/// A small refresh button for the top-right of a card or page.
pub fn refresh_button(ui: &mut Ui, busy: bool) -> bool {
    if busy {
        ui.add(egui::Spinner::new().color(widgets::palette(ui).primary));
        false
    } else {
        widgets::icon_button(ui, ICON_REFRESH, "Refresh", true).clicked()
    }
}

/// Label/value pairs in two columns (used by About and status cards).
pub fn facts(ui: &mut Ui, id: &str, items: &[(&str, String)]) {
    let p = widgets::palette(ui);
    ui.add_space(8.0);
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([24.0, 8.0])
        .show(ui, |ui| {
            for (k, v) in items {
                ui.label(RichText::new(*k).color(p.on_surface_variant));
                ui.add(egui::Label::new(RichText::new(v).color(p.on_surface)).wrap());
                ui.end_row();
            }
        });
    ui.add_space(8.0);
}

/// A pill-shaped search field like the file manager's toolbar search.
/// Escape clears it.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> egui::Response {
    use egui::{pos2, vec2, Rect, Sense};
    let p = widgets::palette(ui);
    let (rect, bg) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
    let fill = if bg.hovered() {
        widgets::state_layer(p.surface_container_high, p.on_surface, &bg)
    } else {
        p.surface_container_high
    };
    ui.painter().rect_filled(rect, CornerRadius::same(20), fill);
    widgets::paint_icon(ui, ICON_SEARCH, pos2(rect.left() + 20.0, rect.center().y), 20.0, p.on_surface_variant);
    let clear_w = if text.is_empty() { 0.0 } else { 30.0 };
    let inner = Rect::from_min_max(
        pos2(rect.left() + 38.0, rect.center().y - 10.0),
        pos2(rect.right() - 10.0 - clear_w, rect.center().y + 10.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let resp = child.add(
        egui::TextEdit::singleline(text)
            .frame(egui::Frame::NONE)
            .hint_text(hint)
            .font(egui::FontId::proportional(14.0))
            .desired_width(inner.width()),
    );
    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        text.clear();
        resp.surrender_focus();
    }
    if !text.is_empty() {
        let r = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(28.0, 28.0));
        let clear = ui
            .interact(r, resp.id.with("clear"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if clear.hovered() {
            ui.painter().circle_filled(r.center(), 14.0, p.on_surface.gamma_multiply(0.08));
        }
        widgets::paint_icon(ui, ICON_CLOSE, r.center(), 18.0, p.on_surface_variant);
        if clear.clicked() {
            text.clear();
        }
    }
    resp
}

/// A centered modal dialog in the app's style. Returns what `body` returns.
pub fn modal<R>(
    ctx: &egui::Context,
    p: &M3Palette,
    id: &str,
    icon: MaterialIcon,
    title: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> egui::ModalResponse<R> {
    egui::Modal::new(Id::unique(("m3s-modal", id)))
        .backdrop_color(p.scrim.gamma_multiply(0.4))
        .frame(
            Frame::new()
                .fill(p.surface_container_high)
                .corner_radius(CornerRadius::same(28))
                .inner_margin(Margin::same(24))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 32,
                    spread: 0,
                    color: Color32::from_black_alpha(90),
                }),
        )
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                widgets::icon(ui, icon, 26.0, p.secondary);
                ui.add_space(8.0);
                ui.label(RichText::new(title).size(22.0).color(p.on_surface));
            });
            ui.add_space(14.0);
            body(ui)
        })
}

/// Right-aligned dialog buttons on a fixed-height row.
pub fn dialog_buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(18.0);
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        vec2(width, 36.0),
        egui::Layout::right_to_left(egui::Align::Center),
        add,
    );
}

/// A centered hint inside an empty list.
pub fn empty_hint(ui: &mut Ui, text: &str) {
    let p = widgets::palette(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(13.0),
        p.on_surface_variant,
    );
}
