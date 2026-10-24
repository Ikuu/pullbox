//! The Series Check window: missing issues, duplicates and trades that
//! overlap single issues. It isn't modal, so comics it lists can be opened
//! in the library while it stays up.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use egui::RichText;

use crate::archive;
use crate::library::ComicRecord;
use crate::library::series_check::{self, Contents, Report, SeriesReport, format_issues, parse_issue_list};
use crate::theme;

pub enum SeriesCheckOutcome {
    Close,
    /// Save the issues a trade collects; empty forgets them.
    SetCollects(PathBuf, String),
    /// Select this comic in the library.
    Show(PathBuf),
}

enum Event {
    Progress { step: &'static str, done: usize, total: usize },
    Summaries(HashMap<PathBuf, String>),
    Identical(Vec<Vec<PathBuf>>),
}

pub struct SeriesCheckDialog {
    /// What was checked, e.g. "the whole library".
    scope: String,
    comics: Vec<ComicRecord>,
    entered: HashMap<PathBuf, String>,
    summaries: HashMap<PathBuf, String>,
    identical: Vec<Vec<PathBuf>>,
    report: Report,
    loading: Option<Receiver<Event>>,
    progress: Option<(&'static str, usize, usize)>,
    only_problems: bool,
    /// The trade whose issues are being entered, and the text so far.
    editing: Option<(PathBuf, String)>,
}

impl SeriesCheckDialog {
    pub fn new(scope: String, comics: Vec<ComicRecord>, entered: HashMap<PathBuf, String>, ctx: &egui::Context) -> Self {
        let trades = series_check::trades_needing_summaries(&comics, &entered);
        let files: Vec<(PathBuf, u64)> = comics.iter().map(|c| (c.path.clone(), c.size)).collect();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let send = |event| {
                let _ = tx.send(event);
                ctx.request_repaint();
            };
            let mut summaries = HashMap::new();
            for (i, path) in trades.iter().enumerate() {
                send(Event::Progress { step: "Reading collected editions", done: i, total: trades.len() });
                // An unreadable trade just stays "contents unknown".
                if let Ok(info) = archive::read_comic_info(path)
                    && let Some(summary) = info.get("Summary")
                {
                    summaries.insert(path.clone(), summary.to_owned());
                }
            }
            send(Event::Summaries(summaries));
            let progress = |done, total| send(Event::Progress { step: "Comparing files of the same size", done, total });
            send(Event::Identical(series_check::identical_files(&files, progress)));
        });

        let mut dialog = Self {
            scope,
            comics,
            entered,
            summaries: HashMap::new(),
            identical: Vec::new(),
            report: Report::default(),
            loading: Some(rx),
            progress: None,
            only_problems: true,
            editing: None,
        };
        dialog.recheck();
        dialog
    }

    /// After the user's entry for a trade was saved.
    pub fn set_entered(&mut self, path: PathBuf, issues: String) {
        if issues.trim().is_empty() {
            self.entered.remove(&path);
        } else {
            self.entered.insert(path, issues);
        }
        self.recheck();
    }

    fn recheck(&mut self) {
        self.report = series_check::check(&self.comics, &self.entered, &self.summaries);
    }

    fn poll(&mut self) {
        let Some(events) = &self.loading else { return };
        let mut summaries = None;
        while let Ok(event) = events.try_recv() {
            match event {
                Event::Progress { step, done, total } => self.progress = Some((step, done, total)),
                Event::Summaries(loaded) => summaries = Some(loaded),
                Event::Identical(groups) => {
                    self.identical = groups;
                    self.loading = None;
                    self.progress = None;
                    break;
                }
            }
        }
        if let Some(loaded) = summaries {
            self.summaries = loaded;
            self.recheck();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<SeriesCheckOutcome> {
        self.poll();
        let mut outcome = None;
        let mut open = true;
        egui::Window::new("Series Check")
            .open(&mut open)
            .default_size([680.0, 560.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                self.header(ui);
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    outcome = self.body(ui);
                });
            });
        if !open {
            outcome = Some(SeriesCheckOutcome::Close);
        }
        outcome
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.weak(format!("Checking {}", self.scope));
        let report = &self.report;
        let attention = report.series.iter().filter(|s| s.has_findings()).count();
        ui.label(format!("{} series checked, {attention} need attention.", report.series.len()));
        if report.untagged + report.unnumbered > 0 {
            ui.weak(format!(
                "Not checked: {} comics without a series name and {} without an issue number.",
                report.untagged, report.unnumbered
            ));
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.only_problems, "Only series that need attention");
            if let Some((step, done, total)) = self.progress {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("{step}… {done}/{total}"));
                    ui.spinner();
                });
            }
        });
    }

    fn body(&mut self, ui: &mut egui::Ui) -> Option<SeriesCheckOutcome> {
        let mut outcome = None;
        let mut shown = 0;
        // Taken out so the series can be drawn while `self.editing` changes.
        let report = std::mem::take(&mut self.report);
        let only_problems = self.only_problems;
        for series in report.series.iter().filter(|s| !only_problems || s.has_findings()) {
            shown += 1;
            ui.add_space(6.0);
            if let Some(o) = self.series(ui, series) {
                outcome = Some(o);
            }
            ui.add_space(6.0);
            ui.separator();
        }
        self.report = report;

        if !self.identical.is_empty() {
            shown += 1;
            ui.add_space(6.0);
            ui.label(RichText::new("Identical files").family(theme::bold()));
            for group in &self.identical {
                ui.add_space(4.0);
                for path in group {
                    if let Some(o) = comic_link(ui, path) {
                        outcome = Some(o);
                    }
                }
            }
        }
        if shown == 0 && self.loading.is_none() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label("Nothing to report: no gaps or duplicates found."));
        }
        outcome
    }

    fn series(&mut self, ui: &mut egui::Ui, series: &SeriesReport) -> Option<SeriesCheckOutcome> {
        let palette = theme::palette();
        let mut outcome = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&series.series).family(theme::bold()));
            let mut details = format!("{} comics", series.comic_count);
            if let Some(highest) = series.highest {
                details += &format!(", up to #{highest}");
            }
            ui.weak(details);
        });

        ui.indent(&series.series, |ui| {
            match (series.missing.is_empty(), series.uncertain) {
                (false, false) => {
                    ui.colored_label(palette.negative, format!("Missing {}", format_issues(&series.missing)));
                }
                (false, true) => {
                    let text = format!("Possibly missing {}: not every trade's contents are known", format_issues(&series.missing));
                    ui.colored_label(palette.neutral, text);
                }
                (true, false) if series.highest.is_some() => {
                    ui.colored_label(palette.positive, "No gaps");
                }
                _ => {}
            }

            for trade in &series.trades {
                if let Some(o) = self.trade(ui, &trade.path, &trade.contents) {
                    outcome = Some(o);
                }
            }
            for (issue, paths) in &series.duplicates {
                ui.colored_label(palette.neutral, format!("#{issue} is here {} times:", paths.len()));
                for path in paths {
                    if let Some(o) = comic_link(ui, path) {
                        outcome = Some(o);
                    }
                }
            }
            for (issue, single, trade) in &series.overlaps {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("#{issue} is also in"));
                    if let Some(o) = comic_link(ui, trade) {
                        outcome = Some(o);
                    }
                    ui.label("as a single issue:");
                    if let Some(o) = comic_link(ui, single) {
                        outcome = Some(o);
                    }
                });
            }
        });
        outcome
    }

    /// A trade and what it collects, with a way to enter or correct that.
    fn trade(&mut self, ui: &mut egui::Ui, path: &Path, contents: &Contents) -> Option<SeriesCheckOutcome> {
        let palette = theme::palette();
        let mut outcome = None;
        let editing = self.editing.as_ref().is_some_and(|(p, _)| p == path);
        ui.horizontal_wrapped(|ui| {
            if let Some(o) = comic_link(ui, path) {
                outcome = Some(o);
            }
            let (text, color, button) = match contents {
                Contents::Entered(issues) => (format!("collects {} (entered)", format_issues(issues)), None, "Edit"),
                Contents::FromSummary(issues) => {
                    (format!("collects {} (from its summary)", format_issues(issues)), None, "Correct")
                }
                Contents::Unknown => ("contents unknown".to_owned(), Some(palette.neutral), "Set Issues…"),
            };
            match color {
                Some(color) => ui.colored_label(color, text),
                None => ui.weak(text),
            };
            if !editing && ui.small_button(button).clicked() {
                let current = self.entered.get(path).cloned().unwrap_or_default();
                self.editing = Some((path.to_path_buf(), current));
            }
        });

        if editing && let Some(o) = self.editor(ui) {
            outcome = Some(o);
        }
        outcome
    }

    fn editor(&mut self, ui: &mut egui::Ui) -> Option<SeriesCheckOutcome> {
        let (path, text) = self.editing.as_mut()?;
        let mut outcome = None;
        let mut done = false;
        ui.horizontal(|ui| {
            ui.label("Issues:");
            let response = ui.add(egui::TextEdit::singleline(text).hint_text("e.g. 1-6, 8").desired_width(160.0));
            let valid = text.trim().is_empty() || parse_issue_list(text).is_some();
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add_enabled(valid, egui::Button::new("Save")).clicked() || (enter && valid) {
                outcome = Some(SeriesCheckOutcome::SetCollects(path.clone(), text.trim().to_owned()));
                done = true;
            }
            if ui.button("Cancel").clicked() {
                done = true;
            }
            if !valid {
                ui.colored_label(ui.visuals().error_fg_color, "Use issue numbers and ranges, e.g. 1-6, 8");
            } else if text.trim().is_empty() {
                ui.weak("Empty uses the summary again");
            }
        });
        if done {
            self.editing = None;
        }
        outcome
    }
}

/// The comic's file name as a link that shows it in the library.
fn comic_link(ui: &mut egui::Ui, path: &Path) -> Option<SeriesCheckOutcome> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let response = ui.link(name).on_hover_text(format!("{}\nClick to show it in the library", path.display()));
    response.clicked().then(|| SeriesCheckOutcome::Show(path.to_path_buf()))
}

