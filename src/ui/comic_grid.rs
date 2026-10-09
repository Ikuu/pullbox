//! The cover grid view. Only the rows on screen are laid out, so it stays
//! fast for large libraries.

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align, Color32, FontId, Rect, Sense, Vec2, pos2, vec2};

use super::comic_actions::{ComicAction, ComicView};
use crate::library::ComicRecord;
use crate::thumbnails::Thumbnails;

const GAP: f32 = 14.0;
const CAPTION_HEIGHT: f32 = 34.0;
/// Covers are drawn in a box of this height-to-width ratio.
const COVER_ASPECT: f32 = 1.5;

pub const MIN_CELL_WIDTH: f32 = 90.0;
pub const MAX_CELL_WIDTH: f32 = 260.0;

pub struct ComicGrid {
    pub cell_width: f32,
}

impl Default for ComicGrid {
    fn default() -> Self {
        Self { cell_width: 140.0 }
    }
}

impl ComicGrid {
    pub fn show(&self, ui: &mut egui::Ui, view: &mut ComicView, thumbnails: &mut Thumbnails) -> Option<ComicAction> {
        let mut action = view.keyboard_shortcuts(ui);

        let cell = vec2(self.cell_width, self.cell_width * COVER_ASPECT + CAPTION_HEIGHT);
        let columns = ((ui.available_width() + GAP) / (cell.x + GAP)).floor().max(1.0) as usize;
        let rows = view.visible.len().div_ceil(columns);

        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show_rows(ui, cell.y, rows, |ui, row_range| {
                for row in row_range {
                    let start = row * columns;
                    let end = (start + columns).min(view.visible.len());
                    ui.horizontal(|ui| {
                        for position in start..end {
                            if let Some(a) = self.cell(ui, view, thumbnails, position, cell) {
                                action = Some(a);
                            }
                        }
                    });
                }
            });
        action
    }

    fn cell(
        &self,
        ui: &mut egui::Ui,
        view: &mut ComicView,
        thumbnails: &mut Thumbnails,
        position: usize,
        size: Vec2,
    ) -> Option<ComicAction> {
        let comic = &view.comics[view.visible[position]];
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());

        if ui.is_rect_visible(rect) {
            let visuals = ui.visuals();
            let painter = ui.painter();
            if view.selection.contains(&comic.path) {
                painter.rect_filled(rect.expand(5.0), 6.0, visuals.selection.bg_fill);
            } else if response.hovered() {
                painter.rect_filled(rect.expand(5.0), 6.0, visuals.widgets.hovered.weak_bg_fill);
            }

            let cover_area = Rect::from_min_size(rect.min, vec2(size.x, size.x * COVER_ASPECT));
            match thumbnails.get(comic) {
                Some(texture) => {
                    let cover = fit_bottom_centre(texture.size_vec2(), cover_area);
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    painter.image(texture.id(), cover, uv, Color32::WHITE);
                }
                None => {
                    painter.rect_filled(cover_area.shrink(6.0), 4.0, visuals.faint_bg_color);
                }
            }

            let caption = caption_layout(comic, size.x, visuals.text_color());
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(caption));
            painter.galley(pos2(rect.center().x, cover_area.max.y + 4.0), galley, visuals.text_color());
        }

        let response = response.on_hover_text(comic.file_name());
        view.item_interaction(&response, position)
    }
}

/// Scales `image` to fit `area`, sitting on its bottom edge like books on a shelf.
fn fit_bottom_centre(image: Vec2, area: Rect) -> Rect {
    let scale = (area.width() / image.x).min(area.height() / image.y);
    let size = image * scale;
    Rect::from_min_size(pos2(area.center().x - size.x / 2.0, area.max.y - size.y), size)
}

/// Up to two centred lines of title, truncated with an ellipsis.
fn caption_layout(comic: &ComicRecord, width: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::single_section(
        comic.display_title(),
        TextFormat::simple(FontId::proportional(12.0), color),
    );
    job.wrap = TextWrapping {
        max_width: width,
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    job.halign = Align::Center;
    job
}
