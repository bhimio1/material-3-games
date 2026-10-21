//! Painting cover art: rounded, center-cropped to fill, with a tinted
//! placeholder while loading.

use super::widgets;
use crate::thumbs::Thumbs;
use egui::{epaint::RectShape, pos2, Color32, CornerRadius, Rect, Ui};
use material_3_ui::icons::MaterialIcon;

/// UV rect that center-crops an image of `size` to the aspect of `rect`.
fn crop_uv(size: [usize; 2], rect: Rect) -> Rect {
    let (w, h) = (size[0] as f32, size[1] as f32);
    if w <= 0.0 || h <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    let target = rect.width() / rect.height().max(1.0);
    let actual = w / h;
    if actual > target {
        let keep = target / actual;
        let off = (1.0 - keep) / 2.0;
        Rect::from_min_max(pos2(off, 0.0), pos2(off + keep, 1.0))
    } else {
        let keep = actual / target;
        let off = (1.0 - keep) / 2.0;
        Rect::from_min_max(pos2(0.0, off), pos2(1.0, off + keep))
    }
}

pub fn paint(
    ui: &Ui,
    thumbs: &mut Thumbs,
    path: Option<&std::path::Path>,
    rect: Rect,
    radius: u8,
    fallback: MaterialIcon,
) {
    let p = widgets::palette(ui);
    let corner = CornerRadius::same(radius);
    let tex = path.and_then(|u| thumbs.get(u));
    match tex {
        Some(tex) => {
            let uv = crop_uv(tex.size(), rect);
            ui.painter().add(
                RectShape::filled(rect, corner, Color32::WHITE).with_texture(tex.id(), uv),
            );
        }
        None => {
            ui.painter().rect_filled(rect, corner, p.surface_container_highest);
            widgets::paint_icon(
                ui,
                fallback,
                rect.center(),
                (rect.width().min(rect.height()) * 0.4).clamp(16.0, 72.0),
                p.on_surface_variant.gamma_multiply(0.6),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_keeps_aspect() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let uv = crop_uv([640, 480], r);
        assert!((uv.width() - 0.75).abs() < 1e-3 && uv.height() == 1.0);
        let uv = crop_uv([100, 100], r);
        assert_eq!(uv, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
    }
}
