//! The ComicVine dialog: auto-matches the selected comics, lets the user
//! review and correct each match, then applies the metadata.

use std::path::PathBuf;

use egui::{RichText, Ui};

use super::widgets;
use crate::comicvine::{self, Event, Issue, Lookup, Request, Session, Volume};

pub type FieldValues = Vec<(&'static str, String)>;

/// A comic to look up.
pub struct Target {
    pub path: PathBuf,
    pub file_name: String,
    pub lookup: Lookup,
}

pub enum ScrapeOutcome {
    /// Metadata to write, per comic.
    Apply(Vec<(PathBuf, FieldValues)>),
    Cancel,
}

enum Status {
    Searching,
    Matched {
        volume: Volume,
        issue: Box<Issue>,
        values: FieldValues,
    },
    NoMatch,
    Failed(String),
}

struct Item {
    path: PathBuf,
    file_name: String,
    /// Series search text, editable by the user.
    query: String,
    number: String,
    status: Status,
    /// Series found by the last search, for the user to pick from.
    volumes: Vec<Volume>,
    searching_volumes: bool,
    /// Whether to apply this item's match.
    apply: bool,
}

pub struct ComicVineDialog {
    session: Session,
    items: Vec<Item>,
    selected: usize,
    /// Text of the button that applies the matches.
    apply_label: &'static str,
}

/// Fields shown in the match preview.
const PREVIEW_FIELDS: [&str; 8] = ["Series", "Volume", "Number", "Title", "Year", "Publisher", "Writer", "Penciller"];
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

    fn volumes_height(self) -> f32 {
        (self.body_height * 0.3).min(170.0)
    }

    fn cover_height(self) -> f32 {
        (self.body_height * 0.45).min(260.0)
    }
}

impl ComicVineDialog {
    pub fn new(targets: Vec<Target>, api_key: String, apply_label: &'static str, ctx: &egui::Context) -> Self {
        let ctx = ctx.clone();
        let session = Session::start(api_key, move || ctx.request_repaint());

        let items = targets
            .into_iter()
            .enumerate()
            .map(|(target, Target { path, file_name, lookup })| {
                let item = Item {
                    path,
                    file_name,
                    query: lookup.series.clone(),
                    number: lookup.number.clone(),
                    status: Status::Searching,
                    volumes: Vec::new(),
                    searching_volumes: true,
                    apply: false,
                };
                session.send(Request::AutoMatch { target, lookup });
                item
            })
            .collect();

        Self { session, items, selected: 0, apply_label }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<ScrapeOutcome> {
        self.receive_events();

        let sizes = Sizes::for_screen(ctx.content_rect());
        let modal = egui::Modal::new(egui::Id::new("comicvine_dialog")).show(ctx, |ui| {
            ui.set_width(sizes.width);
            ui.horizontal(|ui| {
                ui.heading("ComicVine");
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
                // Scrolls if the details don't fit, e.g. on a short window.
                // The explicit vertical layout matters: a scroll area inherits
                // the surrounding (horizontal) layout otherwise.
                let details_width = ui.available_width();
                egui::ScrollArea::vertical()
                    .id_salt("comicvine_details")
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
                Event::Volumes { target, volumes } => {
                    let item = &mut self.items[target];
                    item.volumes = volumes;
                    item.searching_volumes = false;
                }
                Event::Matched { target, volume, issue } => {
                    let values = comicvine::comicinfo_values(&volume, &issue);
                    let item = &mut self.items[target];
                    item.status = Status::Matched { volume, issue, values };
                    item.apply = true;
                }
                Event::NoMatch { target } => self.set_unmatched(target, Status::NoMatch),
                Event::Failed { target, error } => {
                    self.items[target].searching_volumes = false;
                    self.set_unmatched(target, Status::Failed(error));
                }
            }
        }
    }

    fn set_unmatched(&mut self, target: usize, status: Status) {
        let item = &mut self.items[target];
        item.status = status;
        item.apply = false;
    }

    fn progress_text(&self) -> String {
        let pending = self.items.iter().filter(|i| matches!(i.status, Status::Searching)).count();
        let matched = self.items.iter().filter(|i| matches!(i.status, Status::Matched { .. })).count();
        let mut text = format!("{matched} of {} matched", self.items.len());
        if pending > 0 {
            text += &format!(", {pending} searching…");
        }
        text
    }

    fn item_list(&mut self, ui: &mut Ui, sizes: Sizes) {
        egui::ScrollArea::vertical()
            .id_salt("comicvine_items")
            .max_height(sizes.body_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (index, item) in self.items.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let matched = matches!(item.status, Status::Matched { .. });
                        ui.add_enabled(matched, egui::Checkbox::without_text(&mut item.apply))
                            .on_hover_text("Apply this match");
                        ui.vertical(|ui| {
                            let label = ui.add(widgets::selectable(self.selected == index, &item.file_name));
                            if label.clicked() {
                                self.selected = index;
                            }
                            ui.label(status_line(&item.status));
                        });
                    });
                    ui.add_space(2.0);
                }
            });
    }

    fn item_details(&mut self, ui: &mut Ui, sizes: Sizes) {
        let Some(item) = self.items.get_mut(self.selected) else { return };
        let target = self.selected;

        ui.horizontal(|ui| {
            ui.label("Series");
            let query = ui.add(egui::TextEdit::singleline(&mut item.query).desired_width(300.0));
            ui.label("#");
            ui.add(egui::TextEdit::singleline(&mut item.number).desired_width(50.0));
            let enter = query.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Search").clicked() || enter) && !item.query.trim().is_empty() {
                item.searching_volumes = true;
                self.session.send(Request::Search { target, query: item.query.trim().to_owned() });
            }
        });

        ui.add_space(4.0);
        ui.label(RichText::new("Series found — hover for a cover, click to use:").small().weak());
        egui::ScrollArea::vertical()
            .id_salt("comicvine_volumes")
            .max_height(sizes.volumes_height())
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if item.searching_volumes {
                    ui.spinner();
                }
                let current = match &item.status {
                    Status::Matched { volume, .. } => Some(volume.id),
                    _ => None,
                };
                for volume in &item.volumes {
                    let response = ui
                        .add(widgets::selectable(current == Some(volume.id), volume_label(volume)))
                        .on_hover_ui(|ui| {
                            if let Some(image) = &volume.image {
                                ui.add(egui::Image::new(&image.small_url).max_height(260.0));
                            }
                        });
                    if response.clicked() {
                        item.status = Status::Searching;
                        self.session.send(Request::MatchInVolume {
                            target,
                            volume: volume.clone(),
                            number: item.number.trim().to_owned(),
                        });
                    }
                }
            });

        ui.separator();
        match &item.status {
            Status::Searching => {
                ui.spinner();
            }
            Status::Matched { issue, values, .. } => match_preview(ui, issue, values, sizes.cover_height()),
            Status::NoMatch => {
                ui.label(format!(
                    "No issue #{} found. Pick a different series above, or change the search.",
                    item.number
                ));
            }
            Status::Failed(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }
    }

    fn footer(&mut self, ui: &mut Ui) -> Option<ScrapeOutcome> {
        let mut outcome = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Only fields ComicVine has values for are changed.").small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let count = self.items.iter().filter(|i| i.apply).count();
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
            .filter_map(|item| match &item.status {
                Status::Matched { values, .. } => Some((item.path.clone(), values.clone())),
                _ => None,
            })
            .collect()
    }
}

fn status_line(status: &Status) -> RichText {
    let text = match status {
        Status::Searching => "⏳ Searching…".to_owned(),
        Status::Matched { volume, issue, .. } => {
            let year = volume.start_year().map(|y| format!(" ({y})")).unwrap_or_default();
            format!("✔ {}{year} #{}", volume.name, issue.issue_number)
        }
        Status::NoMatch => "✖ No match".to_owned(),
        Status::Failed(_) => "⚠ Error".to_owned(),
    };
    RichText::new(text).small()
}

fn volume_label(volume: &Volume) -> String {
    let year = volume.start_year().map(|y| format!(" ({y})")).unwrap_or_default();
    let publisher = match volume.publisher_name() {
        "" => String::new(),
        name => format!(" — {name}"),
    };
    format!("{}{year}{publisher} — {} issues", volume.name, volume.count_of_issues)
}

fn match_preview(ui: &mut Ui, issue: &Issue, values: &FieldValues, cover_height: f32) {
    let value = |field: &str| values.iter().find(|(f, _)| *f == field).map_or("", |(_, v)| v.as_str());

    ui.horizontal_top(|ui| {
        if let Some(image) = &issue.image {
            let cover = egui::Image::new(&image.small_url).max_height(cover_height).max_width(cover_height / 1.5);
            ui.add(cover);
        }
        ui.vertical(|ui| {
            egui::Grid::new("comicvine_preview").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                for field in PREVIEW_FIELDS {
                    ui.label(RichText::new(field).weak());
                    ui.label(value(field));
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
            if !issue.site_detail_url.is_empty() {
                ui.hyperlink_to("View on ComicVine", &issue.site_detail_url);
            }
        });
    });
}
