//! The ComicVine dialog: auto-matches the selected comics, lets the user
//! review and correct each match, then applies the metadata.

use std::collections::BTreeMap;
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
    /// Narrow the series found to one start year or publisher.
    year_filter: Option<i32>,
    publisher_filter: Option<String>,
    /// Whether to apply this item's match.
    apply: bool,
}

impl Item {
    /// Clears filters that no series found matches any more.
    fn drop_stale_filters(&mut self) {
        if let Some(year) = self.year_filter
            && !self.volumes.iter().any(|v| v.start_year() == Some(year))
        {
            self.year_filter = None;
        }
        if let Some(publisher) = &self.publisher_filter
            && !self.volumes.iter().any(|v| v.publisher_name() == publisher)
        {
            self.publisher_filter = None;
        }
    }
}

pub struct ComicVineDialog {
    session: Session,
    items: Vec<Item>,
    selected: usize,
    /// Text of the button that applies the matches.
    apply_label: &'static str,
    /// Publishers whose series are hidden and skipped by auto-matching.
    ignored_publishers: Vec<String>,
    /// Whether `ignored_publishers` changed since the app last saved it.
    ignored_changed: bool,
    /// Height of the last match preview shown, measured so the results can
    /// take the rest.
    preview_height: Option<f32>,
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

    /// The separator between the results and the match preview, with slack
    /// so the details don't need to scroll.
    const SEPARATOR_HEIGHT: f32 = 20.0;

    /// The results get whatever the rows above them (`above`) and the match
    /// preview leave.
    fn volumes_height(self, above: f32, preview: f32) -> f32 {
        (self.body_height - above - preview - Self::SEPARATOR_HEIGHT).max(150.0)
    }

    fn cover_height(self) -> f32 {
        (self.body_height * 0.45).min(260.0)
    }
}

impl ComicVineDialog {
    pub fn new(
        targets: Vec<Target>,
        api_key: String,
        ignored_publishers: Vec<String>,
        apply_label: &'static str,
        ctx: &egui::Context,
    ) -> Self {
        let ctx = ctx.clone();
        let session = Session::start(api_key, ignored_publishers.clone(), move || ctx.request_repaint());

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
                    year_filter: None,
                    publisher_filter: None,
                    apply: false,
                };
                session.send(Request::AutoMatch { target, lookup });
                item
            })
            .collect();

        Self {
            session,
            items,
            selected: 0,
            apply_label,
            ignored_publishers,
            ignored_changed: false,
            preview_height: None,
        }
    }

    /// The ignored publishers, if the user changed them since the last call.
    pub fn take_ignored_publishers(&mut self) -> Option<Vec<String>> {
        std::mem::take(&mut self.ignored_changed).then(|| self.ignored_publishers.clone())
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
                    item.drop_stale_filters();
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

    fn toggle_ignored(&mut self, publisher: String) {
        if self.ignored_publishers.contains(&publisher) {
            self.ignored_publishers.retain(|p| *p != publisher);
        } else {
            for item in &mut self.items {
                if item.publisher_filter.as_ref() == Some(&publisher) {
                    item.publisher_filter = None;
                }
            }
            self.ignored_publishers.push(publisher);
            self.ignored_publishers.sort_by_key(|p| p.to_lowercase());
        }
        self.session.set_ignored_publishers(self.ignored_publishers.clone());
        self.ignored_changed = true;
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
        let top = ui.cursor().top();

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

        let ignored = &self.ignored_publishers;
        let mut visible: Vec<&Volume> = item.volumes.iter().filter(|v| !v.publisher_ignored(ignored)).collect();
        // Newest first; series without a start year last. Stable, so ties keep
        // ComicVine's relevance order.
        visible.sort_by_key(|v| std::cmp::Reverse(v.start_year()));
        let hidden = item.volumes.len() - visible.len();
        let years = counts(visible.iter().filter_map(|v| v.start_year()));
        let publishers = counts(visible.iter().map(|v| v.publisher_name()).filter(|p| !p.is_empty()));
        let mut ignore_choices: Vec<&str> = item.volumes.iter().map(|v| v.publisher_name()).filter(|p| !p.is_empty()).collect();
        ignore_choices.extend(ignored.iter().map(String::as_str));
        ignore_choices.sort_by_key(|p| p.to_lowercase());
        ignore_choices.dedup();
        let mut toggle_ignored = None;

        ui.horizontal(|ui| {
            ui.label("Year");
            egui::ComboBox::from_id_salt("comicvine_year_filter")
                .width(90.0)
                .selected_text(item.year_filter.map_or("Any".to_owned(), |y| y.to_string()))
                .show_ui(ui, |ui| {
                    widgets::selectable_value(ui, &mut item.year_filter, None, "Any");
                    for &(year, count) in years.iter().rev() {
                        widgets::selectable_value(ui, &mut item.year_filter, Some(year), format!("{year} ({count})"));
                    }
                });
            ui.add_space(8.0);
            ui.label("Publisher");
            egui::ComboBox::from_id_salt("comicvine_publisher_filter")
                .width(180.0)
                .selected_text(item.publisher_filter.as_deref().unwrap_or("Any"))
                .show_ui(ui, |ui| {
                    widgets::selectable_value(ui, &mut item.publisher_filter, None, "Any");
                    for &(publisher, count) in &publishers {
                        let value = Some(publisher.to_owned());
                        widgets::selectable_value(ui, &mut item.publisher_filter, value, format!("{publisher} ({count})"));
                    }
                });
            ui.add_space(8.0);
            let ignore_label = match ignored.len() {
                0 => "Ignore Publishers".to_owned(),
                n => format!("Ignored Publishers ({n})"),
            };
            ui.add_enabled_ui(!ignore_choices.is_empty(), |ui| {
                // Stays open, so several publishers can be toggled in one go.
                let config = egui::containers::menu::MenuConfig::new()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                egui::containers::menu::MenuButton::new(ignore_label).config(config).ui(ui, |ui| {
                    ui.label(RichText::new("Hidden from results and auto-matching:").small().weak());
                    for publisher in &ignore_choices {
                        let mut is_ignored = ignored.iter().any(|p| p == publisher);
                        if ui.checkbox(&mut is_ignored, *publisher).changed() {
                            toggle_ignored = Some(publisher.to_string());
                        }
                    }
                });
            });
        });

        ui.add_space(4.0);
        let mut hint = "Series found — hover for a cover, click to use, right-click to ignore its publisher".to_owned();
        if hidden > 0 {
            hint += &format!(" ({hidden} from ignored publishers hidden)");
        }
        ui.label(RichText::new(hint + ":").small().weak());
        let above = ui.cursor().top() - top;
        let preview_height = self.preview_height.unwrap_or(sizes.cover_height());
        egui::ScrollArea::vertical()
            .id_salt("comicvine_volumes")
            .max_height(sizes.volumes_height(above, preview_height))
            // A fixed height, so the preview below doesn't jump as results change.
            .auto_shrink(false)
            .show(ui, |ui| {
                // Old results would look like the new ones until they arrive.
                if item.searching_volumes {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Searching…").weak());
                    });
                    return;
                }
                let current = match &item.status {
                    Status::Matched { volume, .. } => Some(volume.id),
                    _ => None,
                };
                let shown = visible.iter().filter(|v| {
                    item.year_filter.is_none_or(|year| v.start_year() == Some(year))
                        && item.publisher_filter.as_ref().is_none_or(|p| v.publisher_name() == p)
                });
                let mut any_shown = false;
                for volume in shown {
                    any_shown = true;
                    let response = ui
                        .add(widgets::selectable(current == Some(volume.id), volume_label(volume)))
                        .on_hover_ui(|ui| {
                            if let Some(image) = &volume.image {
                                ui.add(egui::Image::new(&image.small_url).max_height(260.0));
                            }
                        });
                    if !volume.publisher_name().is_empty() {
                        response.context_menu(|ui| {
                            if ui.button(format!("Ignore {}", volume.publisher_name())).clicked() {
                                toggle_ignored = Some(volume.publisher_name().to_owned());
                            }
                        });
                    }
                    if response.clicked() {
                        item.status = Status::Searching;
                        self.session.send(Request::MatchInVolume {
                            target,
                            volume: (*volume).clone(),
                            number: item.number.trim().to_owned(),
                        });
                    }
                }
                if !any_shown && !item.volumes.is_empty() {
                    ui.label(RichText::new("No series match the filters.").weak());
                }
            });

        ui.separator();
        let preview_top = ui.cursor().top();
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

        if matches!(item.status, Status::Matched { .. }) {
            widgets::remember_height(ui, &mut self.preview_height, preview_top);
        }

        if let Some(publisher) = toggle_ignored {
            self.toggle_ignored(publisher);
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

/// Distinct values with how often each occurs, in ascending order.
fn counts<T: Ord>(values: impl Iterator<Item = T>) -> Vec<(T, usize)> {
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_insert(0) += 1;
    }
    counts.into_iter().collect()
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
            // Long values (titles, credit lists) wrap beside the cover rather
            // than widening the dialog. Leaves room for the label column.
            let value_width = (ui.available_width() - 80.0).max(100.0);
            egui::Grid::new("comicvine_preview")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .max_col_width(value_width)
                .show(ui, |ui| {
                    for field in PREVIEW_FIELDS {
                        ui.label(RichText::new(field).weak());
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
            if !issue.site_detail_url.is_empty() {
                ui.hyperlink_to("View on ComicVine", &issue.site_detail_url);
            }
        });
    });
}
