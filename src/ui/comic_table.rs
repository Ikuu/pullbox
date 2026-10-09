//! The detail list view: sortable columns.

use std::borrow::Cow;
use std::cmp::Ordering;

use egui_extras::{Column, TableBuilder};

use super::comic_actions::{ComicAction, ComicView};
use crate::library::ComicRecord;
use crate::util::{format_bytes, format_date_time, natural_cmp};

#[derive(Clone, Copy, PartialEq, Eq)]
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
    ColumnDef { key: SortKey::Compressed, title: "Compressed", width: 80.0 },
    ColumnDef { key: SortKey::Format, title: "Format", width: 55.0 },
    ColumnDef { key: SortKey::Size, title: "Size", width: 75.0 },
    ColumnDef { key: SortKey::Added, title: "Added", width: 115.0 },
    ColumnDef { key: SortKey::File, title: "File", width: 260.0 },
];

const ROW_HEIGHT: f32 = 22.0;

/// What happened in the table this frame.
pub struct TableResponse {
    pub action: Option<ComicAction>,
    /// The sort column or direction changed; the visible list needs re-sorting.
    pub sort_changed: bool,
}

pub struct ComicTable {
    sort_key: SortKey,
    ascending: bool,
}

impl Default for ComicTable {
    fn default() -> Self {
        Self { sort_key: SortKey::Series, ascending: true }
    }
}

impl ComicTable {
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

    pub fn show(&mut self, ui: &mut egui::Ui, view: &mut ComicView) -> TableResponse {
        let mut action = view.keyboard_shortcuts(ui);
        let mut sort_changed = false;

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for (i, col) in COLUMNS.iter().enumerate() {
            let column = if i == COLUMNS.len() - 1 {
                Column::remainder()
            } else {
                Column::initial(col.width).at_least(30.0)
            };
            table = table.column(column.clip(true));
        }

        table
            .header(ROW_HEIGHT, |mut header| {
                for col in &COLUMNS {
                    header.col(|ui| sort_changed |= self.header_button(ui, col));
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, view.visible.len(), |mut row| {
                    let position = row.index();
                    let comic = &view.comics[view.visible[position]];
                    row.set_selected(view.selection.contains(&comic.path));

                    for col in &COLUMNS {
                        row.col(|ui| {
                            let text = match col.key {
                                SortKey::Compressed => compressed_mark(comic.compressed),
                                // Untagged comics would otherwise be blank rows.
                                SortKey::Series if comic.meta.series.is_empty() => {
                                    egui::RichText::new(comic.display_title()).weak().italics()
                                }
                                key => egui::RichText::new(cell_text(key, comic)),
                            };
                            ui.add(egui::Label::new(text).truncate().selectable(false));
                        });
                    }

                    if let Some(a) = view.item_interaction(&row.response(), position) {
                        action = Some(a);
                    }
                });
            });

        TableResponse { action, sort_changed }
    }

    /// Draws a clickable column header. Returns true if the sort order changed.
    fn header_button(&mut self, ui: &mut egui::Ui, col: &ColumnDef) -> bool {
        let is_sorted = self.sort_key == col.key;
        let text = match (is_sorted, self.ascending) {
            (true, true) => format!("{} ⏶", col.title),
            (true, false) => format!("{} ⏷", col.title),
            (false, _) => col.title.to_owned(),
        };
        let clicked = ui
            .add(egui::Button::new(egui::RichText::new(text).strong()).frame(false))
            .clicked();
        if clicked {
            if is_sorted {
                self.ascending = !self.ascending;
            } else {
                self.sort_key = col.key;
                self.ascending = true;
            }
        }
        clicked
    }
}

/// A green tick or red cross.
fn compressed_mark(compressed: bool) -> egui::RichText {
    if compressed {
        egui::RichText::new("✔").color(egui::Color32::from_rgb(90, 190, 110))
    } else {
        egui::RichText::new("✖").color(egui::Color32::from_rgb(210, 90, 90))
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
