//! Small widgets used in more than one place.

use egui::{Align2, Color32, FontId, Rect, Stroke, pos2, vec2};

use crate::icons;
use crate::theme;

/// A single-line text box with a search icon inside it, on the left.
pub fn search_field(ui: &mut egui::Ui, text: &mut String, id_salt: &str, hint: &str, width: f32) -> egui::Response {
    let icon = icons::image("edit-find");
    let hint = if icon.is_some() { hint.to_owned() } else { format!("🔍 {hint}") };
    let left = if icon.is_some() { 10 + icons::SIZE as i8 } else { 8 };
    let response = ui.add(
        egui::TextEdit::singleline(text)
            // A fixed id keeps keyboard focus if widgets appear next to it.
            .id_salt(id_salt)
            .hint_text(hint)
            .margin(egui::Margin { left, right: 8, top: 4, bottom: 4 })
            .desired_width(width),
    );
    if let Some(icon) = icon {
        let rect = response.rect;
        let at = pos2(rect.left() + 7.0, rect.center().y - icons::SIZE / 2.0);
        icon.tint(Color32::from_white_alpha(170)).paint_at(ui, Rect::from_min_size(at, vec2(icons::SIZE, icons::SIZE)));
    }
    response
}

/// Paints a small rounded label, placed at `pos` by `align`. Returns its rect.
pub fn paint_badge(
    painter: &egui::Painter,
    pos: egui::Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    fg: Color32,
    bg: Color32,
) -> Rect {
    let galley = painter.layout_no_wrap(text.to_owned(), font, fg);
    let size = galley.size() + vec2(10.0, 4.0);
    let rect = align.anchor_size(pos, size);
    painter.rect(rect, 4.0, bg, Stroke::NONE, egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(5.0, 2.0), galley, fg);
    rect
}

/// A small, dim, upper-case heading that starts a section.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(egui::RichText::new(title.to_uppercase()).small().family(theme::bold()).weak());
    ui.add_space(2.0);
}
