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

/// A selectable button that keeps its size when hovered.
pub fn selectable<'a>(selected: bool, atoms: impl egui::IntoAtoms<'a>) -> Selectable<'a> {
    Selectable { button: egui::Button::selectable(selected, atoms), selected }
}

pub struct Selectable<'a> {
    button: egui::Button<'a>,
    selected: bool,
}

impl Selectable<'_> {
    pub fn gap(mut self, gap: f32) -> Self {
        self.button = self.button.gap(gap);
        self
    }
}

impl egui::Widget for Selectable<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        if self.selected {
            return ui.add(self.button);
        }
        // Unframed, egui still takes the inactive stroke width off the
        // padding, so the button is a little smaller than when hovering
        // draws the frame, and it and everything after it shift. Without
        // that stroke the padding matches the framed size.
        let inactive = &mut ui.visuals_mut().widgets.inactive.bg_stroke.width;
        let width = std::mem::replace(inactive, 0.0);
        let response = ui.add(self.button);
        ui.visuals_mut().widgets.inactive.bg_stroke.width = width;
        response
    }
}

/// Like `Ui::selectable_value`, using [`selectable`].
pub fn selectable_value<'a, T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    atoms: impl egui::IntoAtoms<'a>,
) -> egui::Response {
    let mut response = ui.add(selectable(*current == value, atoms));
    if response.clicked() && *current != value {
        *current = value;
        response.mark_changed();
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

/// The top of a window: its title on the left, a close button on the right,
/// then a separator. Used instead of egui's title bar, which is much taller
/// than the headings on the other dialogs. Returns true if the close button
/// was clicked.
pub fn window_header(ui: &mut egui::Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.heading(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = icons::button("window-close", "🗙", "").frame(false);
            close = ui.add(button).on_hover_text("Close").clicked();
        });
    });
    ui.separator();
    close
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rects of a selectable row and a label below it, with the pointer at `pointer`.
    fn layout(add: &dyn Fn(&mut egui::Ui) -> egui::Response, pointer: egui::Pos2) -> (Rect, Rect) {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| {
            // Like the app's theme: a stroke on inactive buttons, no hover expansion.
            style.visuals.widgets.inactive.bg_stroke.width = 1.0;
            style.visuals.widgets.hovered.bg_stroke.width = 1.0;
            style.visuals.widgets.hovered.expansion = 0.0;
        });
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 300.0))),
            events: vec![egui::Event::PointerMoved(pointer)],
            ..Default::default()
        };
        let mut rects = (Rect::NOTHING, Rect::NOTHING);
        // Hovering takes effect a frame after the pointer arrives.
        for _ in 0..3 {
            let mut output = ctx.run_ui(input.clone(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let row = add(ui).rect;
                    rects = (row, ui.label("below").rect);
                });
            });
            output.textures_delta.clear();
        }
        rects
    }

    #[test]
    fn selectable_keeps_its_size_when_hovered() {
        for selected in [false, true] {
            let add = |ui: &mut egui::Ui| ui.add(selectable(selected, "Absolute Batman"));
            let (row, below) = layout(&add, pos2(390.0, 290.0));
            let (hovered_row, hovered_below) = layout(&add, row.center());
            assert_eq!(row, hovered_row, "selected: {selected}");
            assert_eq!(below, hovered_below, "selected: {selected}");
        }
    }
}
