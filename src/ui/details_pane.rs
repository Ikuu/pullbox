//! The right-hand pane: cover, credits and summary of the selected comic.
//!
//! The library only indexes a few fields, so the rest are read from the
//! comic's ComicInfo.xml on a background thread when it's selected.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use egui::{CornerRadius, RichText, Shadow, vec2};

use super::ComicAction;
use super::widgets::section;
use crate::archive;
use crate::comicinfo::ComicInfo;
use crate::icons;
use crate::library::ComicRecord;
use crate::thumbnails::Thumbnails;
use crate::util::{format_bytes, format_date_time};

const MAX_COVER_WIDTH: f32 = 240.0;
const COVER_ASPECT: f32 = 1.5;

/// Credits, in the order they're listed.
const CREDITS: [(&str, &str); 8] = [
    ("Writer", "Writer"),
    ("Penciller", "Penciller"),
    ("Inker", "Inker"),
    ("Colorist", "Colorist"),
    ("Letterer", "Letterer"),
    ("CoverArtist", "Cover Artist"),
    ("Editor", "Editor"),
    ("Translator", "Translator"),
];

/// Longer lists, shown as wrapped paragraphs.
const LISTS: [(&str, &str); 5] = [
    ("Characters", "Characters"),
    ("Teams", "Teams"),
    ("Locations", "Locations"),
    ("Genre", "Genre"),
    ("Tags", "Tags"),
];

/// A version of a comic file; a new size or mtime means re-reading it.
#[derive(Clone, PartialEq, Eq)]
struct Key {
    path: PathBuf,
    size: u64,
    modified: i64,
}

impl Key {
    fn of(comic: &ComicRecord) -> Self {
        Self { path: comic.path.clone(), size: comic.size, modified: comic.modified }
    }
}

#[derive(Default)]
pub struct DetailsPane {
    loaded: Option<(Key, Result<ComicInfo, String>)>,
    loading: Option<(Key, Receiver<Result<ComicInfo, String>>)>,
}

impl DetailsPane {
    /// `selected` is in display order.
    pub fn show(&mut self, ui: &mut egui::Ui, selected: &[&ComicRecord], thumbnails: &mut Thumbnails) -> Option<ComicAction> {
        // The pane has a fixed width; long text wraps instead of widening it.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        match selected {
            [] => {
                ui.centered_and_justified(|ui| ui.weak("Select a comic to see its details."));
                None
            }
            [comic] => {
                self.poll(ui.ctx(), comic);
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| self.single(ui, comic, thumbnails))
                    .inner
            }
            many => {
                several(ui, many);
                None
            }
        }
    }

    /// Starts reading the comic's ComicInfo.xml if it isn't loaded or loading.
    fn poll(&mut self, ctx: &egui::Context, comic: &ComicRecord) {
        let key = Key::of(comic);
        if let Some((loading_key, results)) = &self.loading
            && let Ok(result) = results.try_recv()
        {
            self.loaded = Some((loading_key.clone(), result));
            self.loading = None;
        }
        let loaded = self.loaded.as_ref().is_some_and(|(k, _)| *k == key);
        if loaded || self.loading.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }

        let (tx, rx) = channel();
        let path = key.path.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = archive::read_comic_info(&path).map_err(|e| format!("{e:#}"));
            // The pane may have moved on to another comic; that's fine.
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        // A previous, now unwanted, read finishes on its own.
        self.loading = Some((key, rx));
    }

    fn single(&self, ui: &mut egui::Ui, comic: &ComicRecord, thumbnails: &mut Thumbnails) -> Option<ComicAction> {
        let mut action = None;
        cover(ui, comic, thumbnails);
        ui.add_space(12.0);

        let m = &comic.meta;
        let heading = if m.series.is_empty() { comic.display_title() } else { m.series.clone() };
        ui.add(egui::Label::new(RichText::new(heading).heading()).wrap());
        let mut subtitle = Vec::new();
        if !m.number.is_empty() {
            subtitle.push(format!("#{}", m.number));
        }
        // Collected editions are often titled "Vol. 2: …" already.
        if let Some(volume) = m.volume.filter(|_| !m.title.starts_with("Vol")) {
            subtitle.push(format!("Vol. {volume}"));
        }
        if !m.title.is_empty() {
            subtitle.push(m.title.clone());
        }
        if !subtitle.is_empty() {
            ui.add(egui::Label::new(RichText::new(subtitle.join(" · ")).size(14.0)).wrap());
        }
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui.add(icons::button("document-edit", "", "Edit Info…")).clicked() {
                action = Some(ComicAction::EditInfo(vec![comic.path.clone()]));
            }
            let open = icons::button("folder-open", "", "").min_size(vec2(28.0, 0.0));
            if ui.add(open).on_hover_text("Show in File Manager").clicked() {
                action = Some(ComicAction::OpenInFileManager(comic.folder().to_path_buf()));
            }
        });
        ui.add_space(10.0);

        let info = match &self.loaded {
            Some((key, info)) if *key == Key::of(comic) => Some(info),
            _ => None,
        };
        let info = match info {
            Some(Ok(info)) => Some(info),
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("Couldn't read the comic's info: {error}"));
                None
            }
            None if comic.meta.has_info => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Reading…");
                });
                None
            }
            None => None,
        };

        section(ui, "Details");
        egui::Grid::new("details_facts").num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
            let get = |name: &'static str| info.and_then(|i| i.get(name));
            fact(ui, "Publisher", get("Publisher").or(Some(m.publisher.as_str())));
            fact(ui, "Imprint", get("Imprint"));
            fact(ui, "Released", released(info, m.year, m.month).as_deref());
            fact(ui, "Story Arc", get("StoryArc"));
            fact(ui, "Age Rating", get("AgeRating").filter(|r| *r != "Unknown"));
            fact(ui, "Pages", Some(&comic.page_count.to_string()));
            fact(ui, "Format", Some(comic.format.label()));
            fact(ui, "Size", Some(&format_bytes(comic.size)));
            fact(ui, "Compressed", Some(if comic.compressed { "Yes" } else { "No" }));
            fact(ui, "Added", Some(&format_date_time(comic.added)));
        });

        if let Some(info) = info {
            let credits: Vec<_> = CREDITS.iter().filter_map(|(name, label)| Some((*label, info.get(name)?))).collect();
            if !credits.is_empty() {
                section(ui, "Credits");
                egui::Grid::new("details_credits").num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                    for (label, value) in credits {
                        fact(ui, label, Some(value));
                    }
                });
            }
            if let Some(summary) = info.get("Summary") {
                section(ui, "Summary");
                ui.add(egui::Label::new(summary).wrap());
            }
            for (name, label) in LISTS {
                if let Some(value) = info.get(name) {
                    section(ui, label);
                    ui.add(egui::Label::new(value).wrap());
                }
            }
        } else if !comic.meta.has_info {
            ui.add_space(8.0);
            ui.weak("This comic has no ComicInfo.xml. Edit its info or look it up to add one.");
        }

        section(ui, "File");
        ui.add(egui::Label::new(comic.file_name()).wrap());
        ui.add(egui::Label::new(RichText::new(comic.folder().display().to_string()).small().weak()).wrap());
        action
    }
}

/// Summary of a multiple selection.
fn several(ui: &mut egui::Ui, comics: &[&ComicRecord]) {
    ui.add_space(8.0);
    ui.heading(format!("{} comics selected", comics.len()));
    ui.add_space(8.0);
    let first_series = &comics[0].meta.series;
    let same_series = comics.iter().all(|c| c.meta.series == *first_series);
    let size: u64 = comics.iter().map(|c| c.size).sum();
    let pages: u64 = comics.iter().map(|c| u64::from(c.page_count)).sum();
    let uncompressed = comics.iter().filter(|c| !c.compressed).count();
    egui::Grid::new("details_many").num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
        let series = if same_series && !first_series.is_empty() { first_series.as_str() } else { "Various" };
        fact(ui, "Series", Some(series));
        fact(ui, "Total size", Some(&format_bytes(size)));
        fact(ui, "Pages", Some(&pages.to_string()));
        fact(ui, "Uncompressed", Some(&uncompressed.to_string()));
    });
    ui.add_space(12.0);
    ui.weak("Edit Info changes them all at once.");
}

fn cover(ui: &mut egui::Ui, comic: &ComicRecord, thumbnails: &mut Thumbnails) {
    let width = ui.available_width().min(MAX_COVER_WIDTH);
    let texture = thumbnails.get(comic);
    let size = match texture {
        Some(texture) => {
            let image = texture.size_vec2();
            image * (width / image.x).min(width * COVER_ASPECT / image.y)
        }
        None => vec2(width, width * COVER_ASPECT),
    };
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let corners = CornerRadius::same(4);
        match texture {
            Some(texture) => {
                let shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: egui::Color32::from_black_alpha(110) };
                ui.painter().add(shadow.as_shape(rect, corners));
                egui::Image::from_texture(texture).corner_radius(corners).paint_at(ui, rect);
            }
            None => {
                ui.painter().rect_filled(rect, corners, ui.visuals().faint_bg_color);
            }
        }
    });
}

/// "June 2026", or just the year.
fn released(info: Option<&ComicInfo>, year: Option<i32>, month: Option<i32>) -> Option<String> {
    let year = info.and_then(|i| i.get_i32("Year")).or(year)?;
    let month = info.and_then(|i| i.get_i32("Month")).or(month);
    let month = month.and_then(|m| u8::try_from(m).ok()).and_then(|m| chrono::Month::try_from(m).ok());
    Some(match month {
        Some(month) => format!("{} {year}", month.name()),
        None => year.to_string(),
    })
}

/// A label and value on a grid row; nothing if there's no value.
fn fact(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    let Some(value) = value.filter(|v| !v.is_empty()) else { return };
    // The pane wraps by default, and a grid's first column starts out narrow,
    // so a wrapping label would settle at a few characters wide.
    ui.add(egui::Label::new(RichText::new(label).weak()).extend());
    ui.add(egui::Label::new(value).wrap());
    ui.end_row();
}

