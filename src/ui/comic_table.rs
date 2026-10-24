//! The detail list view: sortable columns.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashSet;

use egui_extras::{Column, TableBuilder};

use super::comic_actions::{ComicAction, ComicView};
use crate::icons;
use crate::library::ComicRecord;
use crate::theme;
use crate::util::{format_bytes, format_date_time, natural_cmp};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortKey {
    File,
    Series,
    Number,
    Volume,
    Title,
    Year,
    Publisher,
    Writer,
    Pages,
    Compressed,
    Format,
    Size,
    Added,
}

struct ColumnDef {
    key: SortKey,
    title: &'static str,
    width: f32,
}

const COLUMNS: [ColumnDef; 13] = [
    ColumnDef { key: SortKey::Series, title: "Series", width: 220.0 },
    ColumnDef { key: SortKey::Number, title: "#", width: 45.0 },
    ColumnDef { key: SortKey::Volume, title: "Vol", width: 45.0 },
    ColumnDef { key: SortKey::Title, title: "Title", width: 200.0 },
    ColumnDef { key: SortKey::Year, title: "Year", width: 50.0 },
    ColumnDef { key: SortKey::Publisher, title: "Publisher", width: 120.0 },
    ColumnDef { key: SortKey::Writer, title: "Writer", width: 140.0 },
    ColumnDef { key: SortKey::Pages, title: "Pages", width: 50.0 },
    ColumnDef { key: SortKey::Compressed, title: "Compressed", width: 100.0 },
    ColumnDef { key: SortKey::Format, title: "Format", width: 55.0 },
    ColumnDef { key: SortKey::Size, title: "Size", width: 75.0 },
    ColumnDef { key: SortKey::Added, title: "Added", width: 115.0 },
    ColumnDef { key: SortKey::File, title: "File", width: 260.0 },
];

const ROW_HEIGHT: f32 = 26.0;
const HEADER_HEIGHT: f32 = 28.0;
/// Space between a cell's left edge and its text.
const CELL_PADDING: f32 = 6.0;

/// What happened in the table this frame.
pub struct TableResponse {
    pub action: Option<ComicAction>,
    /// The sort column or direction changed; the visible list needs re-sorting.
    pub sort_changed: bool,
    /// A column was shown or hidden; worth saving.
    pub columns_changed: bool,
}

pub struct ComicTable {
    sort_key: SortKey,
    ascending: bool,
    hidden: HashSet<SortKey>,
}

impl Default for ComicTable {
    fn default() -> Self {
        Self { sort_key: SortKey::Series, ascending: true, hidden: HashSet::new() }
    }
}

impl ComicTable {
    /// Hides the columns with these titles (as saved by `hidden_columns`).
    pub fn set_hidden_columns(&mut self, titles: &[String]) {
        self.hidden = COLUMNS.iter().filter(|c| titles.iter().any(|t| t == c.title)).map(|c| c.key).collect();
    }

    /// Titles of the hidden columns.
    pub fn hidden_columns(&self) -> Vec<String> {
        COLUMNS.iter().filter(|c| self.hidden.contains(&c.key)).map(|c| c.title.to_owned()).collect()
    }

    /// Newest first, e.g. for "Recently Added".
    pub fn sort_newest_first(&mut self) {
        self.sort_key = SortKey::Added;
        self.ascending = false;
    }

    /// Sorts `visible` (indices into `comics`) by the current sort column.
    pub fn sort(&self, comics: &[ComicRecord], visible: &mut [usize]) {
        visible.sort_by(|&a, &b| {
            let (a, b) = (&comics[a], &comics[b]);
            let ord = compare(self.sort_key, a, b)
                // Stable, sensible tie-break for comics in the same series.
                .then_with(|| compare(SortKey::Series, a, b))
                .then_with(|| compare(SortKey::Volume, a, b))
                .then_with(|| compare(SortKey::Number, a, b))
                .then_with(|| compare(SortKey::File, a, b));
            if self.ascending { ord } else { ord.reverse() }
        });
    }

    /// `scroll_to` is a row to bring into view this frame.
    pub fn show(&mut self, ui: &mut egui::Ui, view: &mut ComicView, scroll_to: Option<usize>) -> TableResponse {
        let mut action = view.keyboard_shortcuts(ui);
        let mut sort_changed = false;
        let mut columns_changed = false;
        let columns: Vec<&ColumnDef> = COLUMNS.iter().filter(|c| !self.hidden.contains(&c.key)).collect();

        // Cells touch, so the header and row highlights have no gaps; the
        // text is inset instead.
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        // Column widths are saved by position, so each set of shown columns
        // keeps its own. All columns shown uses the original id, keeping
        // widths saved before columns could be hidden.
        let hidden_mask = COLUMNS.iter().enumerate().filter(|(_, c)| self.hidden.contains(&c.key)).fold(0u32, |m, (i, _)| m | 1 << i);
        if hidden_mask != 0 {
            table = table.id_salt(("comic_table", hidden_mask));
        }
        if let Some(row) = scroll_to {
            table = table.scroll_to_row(row, Some(egui::Align::Center));
        }
        for (i, col) in columns.iter().enumerate() {
            let column = if i == columns.len() - 1 {
                Column::remainder()
            } else {
                Column::initial(col.width).at_least(30.0)
            };
            table = table.column(column.clip(true));
        }

        table
            .header(HEADER_HEIGHT, |mut header| {
                for col in &columns {
                    header.col(|ui| {
                        let response = self.header_button(ui, col, &mut sort_changed);
                        response.context_menu(|ui| columns_changed |= self.column_menu(ui));
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, view.visible.len(), |mut row| {
                    let position = row.index();
                    let comic = &view.comics[view.visible[position]];
                    row.set_selected(view.selection.contains(&comic.path));

                    for col in &columns {
                        row.col(|ui| {
                            let text = match col.key {
                                SortKey::Compressed => compressed_mark(comic.compressed),
                                // Untagged comics would otherwise be blank rows.
                                SortKey::Series if comic.meta.series.is_empty() => {
                                    egui::RichText::new(comic.display_title()).weak().italics()
                                }
                                key => egui::RichText::new(cell_text(key, comic)),
                            };
                            ui.add_space(CELL_PADDING);
                            ui.add(egui::Label::new(text).truncate().selectable(false));
                        });
                    }

                    if let Some(a) = view.item_interaction(&row.response(), position) {
                        action = Some(a);
                    }
                });
            });

        TableResponse { action, sort_changed, columns_changed }
    }

    /// Checkboxes to show and hide columns. Returns true if one changed.
    fn column_menu(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let shown = COLUMNS.len() - self.hidden.len();
        for col in &COLUMNS {
            let mut visible = !self.hidden.contains(&col.key);
            // At least one column stays.
            let enabled = !(visible && shown == 1);
            if ui.add_enabled(enabled, egui::Checkbox::new(&mut visible, col.title)).changed() {
                if visible {
                    self.hidden.remove(&col.key);
                } else {
                    self.hidden.insert(col.key);
                }
                changed = true;
            }
        }
        changed
    }

    /// Draws a clickable column header; sets `sort_changed` if it was clicked.
    fn header_button(&mut self, ui: &mut egui::Ui, col: &ColumnDef, sort_changed: &mut bool) -> egui::Response {
        let is_sorted = self.sort_key == col.key;
        let arrow = match (is_sorted, self.ascending) {
            (false, _) => None,
            (true, true) => Some(("arrow-up", "⏶")),
            (true, false) => Some(("arrow-down", "⏷")),
        };
        let arrow_icon = arrow.and_then(|(name, _)| icons::image(name));
        let text = match arrow {
            Some((_, glyph)) if arrow_icon.is_none() => format!("{} {glyph}", col.title),
            _ => col.title.to_owned(),
        };

        // The whole cell is the button.
        let rect = ui.max_rect();
        let response = ui.interact(rect, ui.id().with("sort"), egui::Sense::click());
        let visuals = ui.visuals();
        let fill = if response.hovered() { visuals.widgets.hovered.weak_bg_fill } else { theme::palette().header_bg };
        ui.painter().rect_filled(rect, 0.0, fill);
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, visuals.widgets.noninteractive.bg_stroke);

        ui.add_space(CELL_PADDING);
        let text = egui::RichText::new(text).family(theme::bold());
        ui.add(egui::Label::new(text).truncate().selectable(false));
        if let Some(icon) = arrow_icon {
            ui.add_space(4.0);
            ui.add(icon.fit_to_exact_size(egui::vec2(12.0, 12.0)));
        }

        if response.clicked() {
            if is_sorted {
                self.ascending = !self.ascending;
            } else {
                self.sort_key = col.key;
                self.ascending = true;
            }
            *sort_changed = true;
        }
        response.on_hover_text("Click to sort, right-click to choose columns")
    }
}

/// A tick or cross in the colour scheme's positive and negative colours.
fn compressed_mark(compressed: bool) -> egui::RichText {
    let palette = theme::palette();
    if compressed {
        egui::RichText::new("✔").color(palette.positive)
    } else {
        egui::RichText::new("✖").color(palette.negative)
    }
}

fn cell_text(key: SortKey, comic: &ComicRecord) -> Cow<'_, str> {
    let m = &comic.meta;
    let opt = |v: Option<i32>| v.map(|v| v.to_string()).unwrap_or_default().into();
    match key {
        SortKey::File => comic.file_name().into(),
        SortKey::Series => m.series.as_str().into(),
        SortKey::Number => m.number.as_str().into(),
        SortKey::Volume => opt(m.volume),
        SortKey::Title => m.title.as_str().into(),
        SortKey::Year => opt(m.year),
        SortKey::Publisher => m.publisher.as_str().into(),
        SortKey::Writer => m.writer.as_str().into(),
        SortKey::Pages => comic.page_count.to_string().into(),
        SortKey::Compressed => (if comic.compressed { "Yes" } else { "No" }).into(),
        SortKey::Format => comic.format.label().into(),
        SortKey::Size => format_bytes(comic.size).into(),
        SortKey::Added => format_date_time(comic.added).into(),
    }
}

fn compare(key: SortKey, a: &ComicRecord, b: &ComicRecord) -> Ordering {
    let (ma, mb) = (&a.meta, &b.meta);
    match key {
        SortKey::File => natural_cmp(&a.file_name(), &b.file_name()),
        SortKey::Series => natural_cmp(&ma.series, &mb.series),
        SortKey::Number => natural_cmp(&ma.number, &mb.number),
        SortKey::Volume => ma.volume.cmp(&mb.volume),
        SortKey::Title => natural_cmp(&ma.title, &mb.title),
        SortKey::Year => (ma.year, ma.month).cmp(&(mb.year, mb.month)),
        SortKey::Publisher => natural_cmp(&ma.publisher, &mb.publisher),
        SortKey::Writer => natural_cmp(&ma.writer, &mb.writer),
        SortKey::Pages => a.page_count.cmp(&b.page_count),
        SortKey::Compressed => a.compressed.cmp(&b.compressed),
        SortKey::Format => a.format.label().cmp(b.format.label()),
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Added => a.added.cmp(&b.added),
    }
}
