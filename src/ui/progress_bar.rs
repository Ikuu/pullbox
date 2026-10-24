//! A progress bar whose label stays readable over both the filled and the
//! empty part. egui's own bar draws all of it in the selection text colour,
//! which can vanish against the empty part (dark text on a dark track).

use egui::{Align2, Rect, Sense, TextStyle, vec2};

const HEIGHT: f32 = 18.0;

/// `fraction` is from 0 to 1.
pub fn progress_bar(ui: &mut egui::Ui, width: f32, fraction: f32, text: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let visuals = ui.visuals();
    let painter = ui.painter();
    let radius = HEIGHT / 2.0;
    painter.rect_filled(rect, radius, visuals.extreme_bg_color);
    painter.rect_stroke(rect, radius, visuals.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);

    let filled = Rect::from_min_size(rect.min, vec2(rect.width() * fraction.clamp(0.0, 1.0), rect.height()));
    // Clipped rather than shrunk, so a nearly empty bar is a sliver, not a dot.
    painter.with_clip_rect(filled).rect_filled(rect, radius, visuals.selection.bg_fill);

    let font = TextStyle::Small.resolve(ui.style());
    let pos = rect.center();
    let empty = Rect::from_min_max(egui::pos2(filled.max.x, rect.min.y), rect.max);
    painter.with_clip_rect(empty).text(pos, Align2::CENTER_CENTER, text, font.clone(), visuals.text_color());
    painter.with_clip_rect(filled).text(pos, Align2::CENTER_CENTER, text, font, visuals.selection.stroke.color);
    response
}
