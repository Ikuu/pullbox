//! The AniList dialog: matches the selected manga to series on AniList, lets
//! the user review and correct each match, then applies the metadata.
//!
//! AniList only knows whole series, so each file's volume and chapter come
//! from its file name or tags, and can be corrected here.

use std::path::PathBuf;

use egui::{RichText, Ui};

use super::comicvine_dialog::{FieldValues, ScrapeOutcome};
use super::widgets;
use crate::anilist::{self, Event, MangaLookup, Media, Request, Session};
use crate::comicvine::normalize_name;

/// A manga file to look up.
pub struct Target {
    pub path: PathBuf,
    pub file_name: String,
    pub lookup: MangaLookup,
}

enum Status {
    Searching,
    Matched(Box<Media>),
    NoMatch,
    Failed(String),
}

struct Item {
    path: PathBuf,
    file_name: String,
    /// Series search text, editable by the user.
    query: String,
    volume: String,
    chapter: String,
    year: Option<i32>,
    status: Status,
    /// Series found by the last search, for the user to pick from.
    results: Vec<Media>,
    searching: bool,
    /// Whether to apply this item's match.
    apply: bool,
}

impl Item {
    fn lookup(&self) -> MangaLookup {
        MangaLookup {
            series: self.query.trim().to_owned(),
            volume: self.volume.trim().parse().ok(),
            chapter: self.chapter.trim().to_owned(),
            year: self.year,
        }
    }

    fn values(&self) -> Option<FieldValues> {
        match &self.status {
            Status::Matched(media) => Some(anilist::comicinfo_values(media, &self.lookup())),
            _ => None,
        }
    }

    fn is_matched(&self) -> bool {
        matches!(self.status, Status::Matched(_))
    }
}

pub struct AniListDialog {
    session: Session,
    items: Vec<Item>,
    selected: usize,
    /// Text of the button that applies the matches.
    apply_label: &'static str,
    /// Height of the last match preview shown, measured so the results can
    /// take the rest.
    preview_height: Option<f32>,
}

/// Fields shown in the match preview, with their labels.
const PREVIEW_FIELDS: [(&str, &str); 9] = [
    ("Series", "Series"),
    ("LocalizedSeries", "Also known as"),
    ("Volume", "Volume"),
    ("Number", "Chapter"),
    ("Count", "Out of"),
    ("Writer", "Story"),
    ("Penciller", "Art"),
    ("Genre", "Genre"),
    ("Tags", "Tags"),
];
const SUMMARY_PREVIEW_CHARS: usize = 600;

/// Sizes derived from the window, so the dialog always fits on screen.
#[derive(Clone, Copy)]
struct Sizes {
    width: f32,
    /// Height available between the header and footer.
    body_height: f32,
}

impl Sizes {
    /// Room for the title, separators and footer buttons.
    const CHROME_HEIGHT: f32 = 150.0;

    fn for_screen(screen: egui::Rect) -> Self {
        Self {
            width: (screen.width() - 60.0).clamp(500.0, 1000.0),
            body_height: (screen.height() - Self::CHROME_HEIGHT).max(200.0),
        }
    }

    fn list_width(self) -> f32 {
        (self.width * 0.34).min(340.0)
    }

    /// The separator between the results and the match preview, with slack
    /// so the details don't need to scroll.
    const SEPARATOR_HEIGHT: f32 = 20.0;

    /// The results get whatever the rows above them (`above`) and the match
    /// preview leave.
    fn results_height(self, above: f32, preview: f32) -> f32 {
        (self.body_height - above - preview - Self::SEPARATOR_HEIGHT).max(150.0)
    }

    fn cover_height(self) -> f32 {
        (self.body_height * 0.45).min(260.0)
    }
}

impl AniListDialog {
    pub fn new(targets: Vec<Target>, apply_label: &'static str, ctx: &egui::Context) -> Self {
        let ctx = ctx.clone();
        let session = Session::start(move || ctx.request_repaint());

        let items = targets
            .into_iter()
            .enumerate()
            .map(|(target, Target { path, file_name, lookup })| {
                let searching = !lookup.series.trim().is_empty();
                if searching {
                    session.send(Request { target, query: lookup.series.clone() });
                }
                Item {
                    path,
                    file_name,
                    query: lookup.series,
                    volume: lookup.volume.map(|v| v.to_string()).unwrap_or_default(),
                    chapter: lookup.chapter,
                    year: lookup.year,
                    status: if searching { Status::Searching } else { Status::NoMatch },
                    results: Vec::new(),
                    searching,
                    apply: false,
                }
            })
            .collect();

        Self { session, items, selected: 0, apply_label, preview_height: None }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<ScrapeOutcome> {
        self.receive_events();

        let sizes = Sizes::for_screen(ctx.content_rect());
        let modal = egui::Modal::new(egui::Id::new("anilist_dialog")).show(ctx, |ui| {
            ui.set_width(sizes.width);
            ui.horizontal(|ui| {
                ui.heading("AniList");
                ui.label(RichText::new(self.progress_text()).weak());
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.set_max_height(sizes.body_height);
                ui.vertical(|ui| {
                    ui.set_width(sizes.list_width());
                    self.item_list(ui, sizes);
                });
                ui.separator();
                // An explicit vertical layout: a scroll area inherits the
                // surrounding (horizontal) layout otherwise.
                let details_width = ui.available_width();
                egui::ScrollArea::vertical()
                    .id_salt("anilist_details")
                    .max_height(sizes.body_height)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(details_width);
                            self.item_details(ui, sizes);
                        });
                    });
            });
            ui.separator();
            self.footer(ui)
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return Some(ScrapeOutcome::Cancel);
        }
        modal.inner
    }

    fn receive_events(&mut self) {
        while let Ok(event) = self.session.events.try_recv() {
            match event {
                Event::Results { target, results } => {
                    let item = &mut self.items[target];
                    item.results = results;
                    item.searching = false;
                    // A new search doesn't undo a match the user already has.
                    if !item.is_matched() {
                        match anilist::best_match(&item.results, &item.lookup()) {
                            Some(media) => {
                                item.status = Status::Matched(Box::new(media.clone()));
                                item.apply = true;
                            }
                            None => item.status = Status::NoMatch,
                        }
                    }
                }
                Event::Failed { target, error } => {
                    let item = &mut self.items[target];
                    item.searching = false;
                    if !item.is_matched() {
                        item.status = Status::Failed(error);
                    }
                }
            }
        }
    }

    /// Uses `media` for the item at `index`, and for the other files
    /// searched for under the same name that have no match yet.
    fn pick(&mut self, index: usize, media: &Media) {
        let key = normalize_name(&self.items[index].query);
        for (i, item) in self.items.iter_mut().enumerate() {
            if i == index || (!item.is_matched() && normalize_name(&item.query) == key) {
                item.status = Status::Matched(Box::new(media.clone()));
                item.apply = true;
            }
        }
    }

    fn progress_text(&self) -> String {
        let pending = self.items.iter().filter(|i| matches!(i.status, Status::Searching)).count();
        let matched = self.items.iter().filter(|i| i.is_matched()).count();
        let mut text = format!("{matched} of {} matched", self.items.len());
        if pending > 0 {
            text += &format!(", {pending} searching…");
        }
        text
    }

    fn item_list(&mut self, ui: &mut Ui, sizes: Sizes) {
        egui::ScrollArea::vertical()
            .id_salt("anilist_items")
            .max_height(sizes.body_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (index, item) in self.items.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_enabled(item.is_matched(), egui::Checkbox::without_text(&mut item.apply))
                            .on_hover_text("Apply this match");
                        ui.vertical(|ui| {
                            let label = ui.add(widgets::selectable(self.selected == index, &item.file_name));
                            if label.clicked() {
                                self.selected = index;
                            }
                            ui.label(status_line(item));
                        });
                    });
                    ui.add_space(2.0);
                }
            });
    }

    fn item_details(&mut self, ui: &mut Ui, sizes: Sizes) {
        let target = self.selected;
        let Some(item) = self.items.get_mut(target) else { return };
        let top = ui.cursor().top();

        ui.horizontal(|ui| {
            ui.label("Series");
            let query = ui.add(egui::TextEdit::singleline(&mut item.query).desired_width(300.0));
            let enter = query.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Search").clicked() || enter) && !item.query.trim().is_empty() {
                item.searching = true;
                self.session.send(Request { target, query: item.query.trim().to_owned() });
            }
        });
        ui.horizontal(|ui| {
            ui.label("Volume");
            ui.add(egui::TextEdit::singleline(&mut item.volume).desired_width(50.0));
            ui.add_space(8.0);
            ui.label("Chapter");
            ui.add(egui::TextEdit::singleline(&mut item.chapter).desired_width(50.0));
            ui.label(RichText::new("Leave Chapter empty for a whole volume.").small().weak());
        });
        if !item.volume.trim().is_empty() && item.volume.trim().parse::<i32>().is_err() {
            ui.colored_label(ui.visuals().error_fg_color, "Volume must be a whole number.");
        }

        ui.add_space(4.0);
        ui.label(RichText::new("Series found — hover for a cover, click to use:").small().weak());
        let above = ui.cursor().top() - top;
        let preview_height = self.preview_height.unwrap_or(sizes.cover_height());
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("anilist_results")
            .max_height(sizes.results_height(above, preview_height))
            // A fixed height, so the preview below doesn't jump as results change.
            .auto_shrink(false)
            .show(ui, |ui| {
                // Old results would look like the new ones until they arrive.
                if item.searching {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Searching…").weak());
                    });
                    return;
                }
                let current = match &item.status {
                    Status::Matched(media) => Some(media.id),
                    _ => None,
                };
                for media in &item.results {
                    let response = ui
                        .add(widgets::selectable(current == Some(media.id), result_label(media)))
                        .on_hover_ui(|ui| {
                            if let Some(url) = media.cover_url() {
                                ui.add(egui::Image::new(url).max_height(260.0));
                            }
                        });
                    if response.clicked() {
                        picked = Some(media.clone());
                    }
                }
            });

        ui.separator();
        let preview_top = ui.cursor().top();
        match &item.status {
            Status::Searching => {
                ui.spinner();
            }
            Status::Matched(media) => {
                let values = item.values().unwrap_or_default();
                match_preview(ui, media, &values, sizes.cover_height());
            }
            Status::NoMatch if item.query.trim().is_empty() => {
                ui.label("Enter the series name above and search.");
            }
            Status::NoMatch => {
                ui.label(format!("No series found for “{}”. Pick one above, or change the search.", item.query.trim()));
            }
            Status::Failed(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }

        if matches!(item.status, Status::Matched(_)) {
            widgets::remember_height(ui, &mut self.preview_height, preview_top);
        }

        if let Some(media) = picked {
            self.pick(target, &media);
        }
    }

    fn footer(&mut self, ui: &mut Ui) -> Option<ScrapeOutcome> {
        let mut outcome = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Only fields AniList has values for are changed.").small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let count = self.items.iter().filter(|i| i.apply && i.is_matched()).count();
                let apply = egui::Button::new(match count {
                    1 => self.apply_label.to_owned(),
                    _ => format!("{} ({count} comics)", self.apply_label),
                });
                if ui.add_enabled(count > 0, apply).clicked() {
                    outcome = Some(ScrapeOutcome::Apply(self.to_apply()));
                }
                if ui.button("Cancel").clicked() {
                    outcome = Some(ScrapeOutcome::Cancel);
                }
            });
        });
        outcome
    }

    fn to_apply(&self) -> Vec<(PathBuf, FieldValues)> {
        self.items
            .iter()
            .filter(|item| item.apply)
            .filter_map(|item| Some((item.path.clone(), item.values()?)))
            .collect()
    }
}

fn status_line(item: &Item) -> RichText {
    let text = match &item.status {
        Status::Searching => "⏳ Searching…".to_owned(),
        Status::Matched(media) => {
            let mut text = format!("✔ {}", media.display_title());
            if let Ok(volume) = item.volume.trim().parse::<i32>() {
                text += &format!(" v{volume}");
            }
            if !item.chapter.trim().is_empty() {
                text += &format!(" ch. {}", item.chapter.trim());
            }
            text
        }
        Status::NoMatch => "✖ No match".to_owned(),
        Status::Failed(_) => "⚠ Error".to_owned(),
    };
    RichText::new(text).small()
}

/// "Berserk (1989) — Manga — 41 volumes".
fn result_label(media: &Media) -> String {
    let year = media.start_date.year.map(|y| format!(" ({y})")).unwrap_or_default();
    let length = match (media.volumes, media.chapters) {
        (Some(1), _) => " — 1 volume".to_owned(),
        (Some(volumes), _) => format!(" — {volumes} volumes"),
        (None, Some(chapters)) => format!(" — {chapters} chapters"),
        (None, None) => String::new(),
    };
    format!("{}{year} — {}{length}", media.display_title(), media.kind_label())
}

fn match_preview(ui: &mut Ui, media: &Media, values: &FieldValues, cover_height: f32) {
    let value = |field: &str| values.iter().find(|(f, _)| *f == field).map_or("", |(_, v)| v.as_str());

    ui.horizontal_top(|ui| {
        if let Some(url) = media.cover_url() {
            ui.add(egui::Image::new(url).max_height(cover_height).max_width(cover_height / 1.5));
        }
        ui.vertical(|ui| {
            // Long values (titles, credit lists) wrap beside the cover rather
            // than widening the dialog. Leaves room for the label column.
            let value_width = (ui.available_width() - 80.0).max(100.0);
            egui::Grid::new("anilist_preview")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .max_col_width(value_width)
                .show(ui, |ui| {
                    for (field, label) in PREVIEW_FIELDS {
                        ui.label(RichText::new(label).weak());
                        ui.add(egui::Label::new(value(field)).wrap());
                        ui.end_row();
                    }
                });
            let summary = value("Summary");
            if !summary.is_empty() {
                ui.add_space(6.0);
                let preview: String = summary.chars().take(SUMMARY_PREVIEW_CHARS).collect();
                let ellipsis = if summary.chars().count() > SUMMARY_PREVIEW_CHARS { "…" } else { "" };
                ui.label(RichText::new(format!("{preview}{ellipsis}")).small());
            }
            if !media.site_url.is_empty() {
                ui.hyperlink_to("View on AniList", &media.site_url);
            }
        });
    });
}
