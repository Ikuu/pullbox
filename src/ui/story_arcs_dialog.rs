//! The Story Arcs dialog: names runs of volumes and chapters in a manga
//! series, then writes each file's arc and its part in it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{Color32, RichText, Stroke, StrokeKind, Ui};

use super::comicvine_dialog::FieldValues;
use super::widgets;
use crate::icons;
use crate::library::ComicRecord;
use crate::library::story_arcs::{self, Arc, Preset, Series};

pub enum ArcsOutcome {
    /// `StoryArc` and `StoryArcNumber` to write, per file.
    Apply(Vec<(PathBuf, FieldValues)>),
    Cancel,
}

struct SeriesState {
    series: Series,
    arcs: Vec<Arc>,
    /// Indices into `series.files`.
    selected: BTreeSet<usize>,
    /// The file last clicked, where a Shift-click range starts.
    anchor: Option<usize>,
}

impl SeriesState {
    fn new(series: Series) -> Self {
        let arcs = story_arcs::arcs_from_tags(&series.files);
        Self { series, arcs, selected: BTreeSet::new(), anchor: None }
    }

    /// Each file's arc index and part, or why the arcs can't be applied.
    fn assignment(&self) -> Result<Vec<Option<(usize, u32)>>, String> {
        story_arcs::assign(&self.arcs, &self.series.files)
    }

    /// What applying would write, for the files whose tags would change.
    fn edits(&self) -> Result<Vec<(PathBuf, FieldValues)>, String> {
        let assignment = self.assignment()?;
        Ok(self
            .series
            .files
            .iter()
            .zip(assignment)
            .filter(|(file, _)| file.writable)
            .filter_map(|(file, assigned)| {
                let (name, part) = match assigned {
                    Some((arc, part)) => (self.arcs[arc].name.trim().to_owned(), part.to_string()),
                    None => (String::new(), String::new()),
                };
                let unchanged = file.story_arc.trim() == name && file.story_arc_number.trim() == part;
                (!unchanged).then(|| (file.path.clone(), vec![("StoryArc", name), ("StoryArcNumber", part)]))
            })
            .collect())
    }

    fn click(&mut self, index: usize, modifiers: egui::Modifiers) {
        match self.anchor {
            Some(anchor) if modifiers.shift => {
                let (start, end) = (anchor.min(index), anchor.max(index));
                if !modifiers.command {
                    self.selected.clear();
                }
                self.selected.extend(start..=end);
                return;
            }
            _ if modifiers.command => {
                if !self.selected.remove(&index) {
                    self.selected.insert(index);
                }
            }
            _ => {
                self.selected.clear();
                self.selected.insert(index);
            }
        }
        self.anchor = Some(index);
    }

    /// Makes the selected files a new arc, taking them out of any other.
    fn arc_from_selection(&mut self) {
        let places = self.selected.iter().map(|&i| &self.series.files[i].place);
        let (volumes, chapters) = story_arcs::numbers_of(places);
        if volumes.is_empty() && chapters.is_empty() {
            return;
        }
        story_arcs::remove_numbers(&mut self.arcs, &volumes, &chapters);
        self.arcs.push(Arc {
            name: format!("Arc {}", self.arcs.len() + 1),
            volumes: story_arcs::format_numbers(&volumes),
            chapters: story_arcs::format_numbers(&chapters),
        });
        story_arcs::sort_arcs(&mut self.arcs);
        self.selected.clear();
        self.anchor = None;
    }
}

pub struct StoryArcsDialog {
    /// What the dialog covers, e.g. "“Berserk” and its subfolders".
    scope: String,
    series: Vec<SeriesState>,
    current: usize,
    /// The outcome of the last preset import or export, and whether it failed.
    message: Option<(String, bool)>,
}

/// Sizes derived from the window, so the dialog always fits on screen.
#[derive(Clone, Copy)]
struct Sizes {
    width: f32,
    body_height: f32,
}

impl Sizes {
    /// Room for the title, series picker, separators and footer.
    const CHROME_HEIGHT: f32 = 180.0;

    fn for_screen(screen: egui::Rect) -> Self {
        Self {
            width: (screen.width() - 60.0).clamp(600.0, 1100.0),
            body_height: (screen.height() - Self::CHROME_HEIGHT).clamp(200.0, 640.0),
        }
    }

    fn arcs_width(self) -> f32 {
        self.width * 0.56
    }
}

impl StoryArcsDialog {
    pub fn new(scope: String, comics: &[ComicRecord]) -> Self {
        let series = story_arcs::series_in(comics).into_iter().map(SeriesState::new).collect();
        Self { scope, series, current: 0, message: None }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<ArcsOutcome> {
        let sizes = Sizes::for_screen(ctx.content_rect());
        let modal = egui::Modal::new(egui::Id::new("story_arcs_dialog")).show(ctx, |ui| {
            ui.set_width(sizes.width);
            ui.horizontal(|ui| {
                ui.heading("Story Arcs");
                ui.label(RichText::new(&self.scope).weak());
            });
            ui.separator();
            if self.series.is_empty() {
                ui.label("There are no comics here.");
            } else {
                self.series_picker(ui);
                ui.add_space(4.0);
                ui.horizontal_top(|ui| {
                    ui.set_max_height(sizes.body_height);
                    ui.vertical(|ui| {
                        ui.set_width(sizes.arcs_width());
                        self.arcs_editor(ui, sizes);
                    });
                    ui.separator();
                    ui.vertical(|ui| self.file_list(ui, sizes));
                });
            }
            ui.separator();
            self.footer(ui)
        });

        // Deliberately not closing on backdrop clicks, so a stray click can't lose edits.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return Some(ArcsOutcome::Cancel);
        }
        modal.inner
    }

    fn series_picker(&mut self, ui: &mut Ui) {
        let state = &self.series[self.current];
        let counts = file_counts(&state.series);
        if self.series.len() == 1 {
            ui.horizontal(|ui| {
                ui.strong(&state.series.name);
                ui.weak(counts);
            });
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Series");
            egui::ComboBox::from_id_salt("story_arcs_series")
                .width(320.0)
                .selected_text(&state.series.name)
                .show_ui(ui, |ui| {
                    for (i, s) in self.series.iter().enumerate() {
                        widgets::selectable_value(ui, &mut self.current, i, &s.series.name);
                    }
                });
            ui.weak(counts);
        });
    }

    fn arcs_editor(&mut self, ui: &mut Ui, sizes: Sizes) {
        let state = &mut self.series[self.current];
        let assignment = state.assignment().ok();
        let count = |arc: usize| {
            assignment.as_ref().map_or("–".to_owned(), |a| {
                a.iter().filter(|x| matches!(x, Some((i, _)) if *i == arc)).count().to_string()
            })
        };

        ui.label(
            RichText::new(
                "List the volumes and chapters each arc spans, like “3-14” or “1-6, 8”. \
                 A chapter's own number decides its arc before its volume's.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);

        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt("story_arcs_list")
            .max_height(sizes.body_height - 60.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let text_width = (sizes.arcs_width() - 230.0).max(200.0);
                egui::Grid::new("story_arcs_grid").num_columns(6).spacing([6.0, 4.0]).show(ui, |ui| {
                    for heading in ["", "Name", "Volumes", "Chapters", "Files", ""] {
                        ui.label(RichText::new(heading).small().weak());
                    }
                    ui.end_row();

                    let last = state.arcs.len().saturating_sub(1);
                    for (i, arc) in state.arcs.iter_mut().enumerate() {
                        ui.label(RichText::new(format!("{}.", i + 1)).weak());
                        ui.add(egui::TextEdit::singleline(&mut arc.name).desired_width(text_width * 0.5));
                        number_list_editor(ui, &mut arc.volumes, text_width * 0.25);
                        number_list_editor(ui, &mut arc.chapters, text_width * 0.25);
                        ui.label(count(i));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let up = ui.add_enabled(i > 0, icons::button("go-up", "⬆", "").frame(false));
                            if up.on_hover_text("Move up").clicked() {
                                action = Some(ArcAction::Swap(i - 1, i));
                            }
                            let down = ui.add_enabled(i < last, icons::button("go-down", "⬇", "").frame(false));
                            if down.on_hover_text("Move down").clicked() {
                                action = Some(ArcAction::Swap(i, i + 1));
                            }
                            let remove = ui.add(icons::button("list-remove", "✖", "").frame(false));
                            if remove.on_hover_text("Remove this arc").clicked() {
                                action = Some(ArcAction::Remove(i));
                            }
                        });
                        ui.end_row();
                    }
                });
                if state.arcs.is_empty() {
                    ui.weak("No arcs yet. Select files on the right and make them an arc, add one, or import a preset.");
                }
            });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.add(icons::button("list-add", "", "Add Arc")).clicked() {
                action = Some(ArcAction::Add);
            }
            let import = ui.add(icons::button("document-import", "", "Import Preset…"));
            if import.on_hover_text("Replace these arcs with ones saved to a file").clicked() {
                action = Some(ArcAction::Import);
            }
            let export = ui.add_enabled(!state.arcs.is_empty(), icons::button("document-export", "", "Export Preset…"));
            if export.on_hover_text("Save these arcs to a file, to reuse or share").clicked() {
                action = Some(ArcAction::Export);
            }
        });

        match action {
            Some(ArcAction::Swap(a, b)) => state.arcs.swap(a, b),
            Some(ArcAction::Remove(i)) => {
                state.arcs.remove(i);
            }
            Some(ArcAction::Add) => {
                let name = format!("Arc {}", state.arcs.len() + 1);
                state.arcs.push(Arc { name, ..Default::default() });
            }
            Some(ArcAction::Import) => self.import_preset(),
            Some(ArcAction::Export) => self.export_preset(),
            None => {}
        }
    }

    fn file_list(&mut self, ui: &mut Ui, sizes: Sizes) {
        let state = &mut self.series[self.current];
        let assignment = state.assignment().ok();

        ui.horizontal(|ui| {
            let selected = state.selected.len();
            let label = match selected {
                0 | 1 => "Make Arc from Selection".to_owned(),
                n => format!("Make Arc from {n} Files"),
            };
            let button = ui.add_enabled(selected > 0, egui::Button::new(label));
            if button.on_disabled_hover_text("Select files first").clicked() {
                state.arc_from_selection();
            }
        });
        ui.label(RichText::new("Click a file, then Shift-click another to select a run.").small().weak());
        ui.add_space(2.0);

        let mut clicked = None;
        egui::ScrollArea::vertical()
            .id_salt("story_arcs_files")
            .max_height(sizes.body_height - 50.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, file) in state.series.files.iter().enumerate() {
                    let response = ui.add(widgets::selectable(state.selected.contains(&i), &file.file_name));
                    if response.clicked() {
                        clicked = Some((i, ui.input(|input| input.modifiers)));
                    }
                    let assigned = assignment.as_ref().and_then(|a| a[i]);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        if !file.place.is_known() {
                            ui.colored_label(ui.visuals().warn_fg_color, RichText::new("No volume or chapter number").small());
                        } else {
                            ui.label(RichText::new(file.place.label()).small().weak());
                        }
                        if let Some((arc, part)) = assigned {
                            ui.label(RichText::new(format!("{} · part {part}", state.arcs[arc].name.trim())).small());
                        }
                        if !file.writable {
                            ui.colored_label(ui.visuals().warn_fg_color, RichText::new("CBR, can't be tagged").small());
                        }
                    });
                }
            });
        if let Some((i, modifiers)) = clicked {
            state.click(i, modifiers);
        }
    }

    fn footer(&mut self, ui: &mut Ui) -> Option<ArcsOutcome> {
        let edits: Result<Vec<_>, String> = self
            .series
            .iter()
            .map(SeriesState::edits)
            .collect::<Result<Vec<_>, _>>()
            .map(|all| all.into_iter().flatten().collect());

        let mut outcome = None;
        ui.horizontal(|ui| {
            match &edits {
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                Ok(edits) if edits.is_empty() => {
                    ui.weak("No changes");
                }
                Ok(edits) => {
                    ui.label(match edits.len() {
                        1 => "1 file will be retagged".to_owned(),
                        n => format!("{n} files will be retagged"),
                    });
                }
            }
            if let Some((message, failed)) = &self.message {
                ui.separator();
                let color = if *failed { ui.visuals().error_fg_color } else { ui.visuals().weak_text_color() };
                ui.colored_label(color, message);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_apply = edits.as_ref().is_ok_and(|e| !e.is_empty());
                if ui.add_enabled(can_apply, egui::Button::new("Apply")).clicked()
                    && let Ok(edits) = &edits
                {
                    outcome = Some(ArcsOutcome::Apply(edits.clone()));
                }
                if ui.button("Cancel").clicked() {
                    outcome = Some(ArcsOutcome::Cancel);
                }
            });
        });
        outcome
    }

    fn import_preset(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import Story Arc Preset")
            .add_filter("Story arc preset", &["json"])
            .pick_file()
        else {
            return;
        };
        let preset = std::fs::read_to_string(&path).map_err(anyhow::Error::from).and_then(|json| Preset::from_json(&json));
        self.message = Some(match preset {
            Ok(preset) => {
                let state = &mut self.series[self.current];
                state.arcs = preset.arcs;
                let count = state.arcs.len();
                (format!("Imported {count} arc(s) for “{}”", preset.series), false)
            }
            Err(e) => (format!("Couldn't import {}: {e:#}", file_name(&path)), true),
        });
    }

    fn export_preset(&mut self) {
        let state = &self.series[self.current];
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export Story Arc Preset")
            .add_filter("Story arc preset", &["json"])
            .set_file_name(format!("{} arcs.json", state.series.name))
            .save_file()
        else {
            return;
        };
        let preset = Preset { series: state.series.name.clone(), arcs: state.arcs.clone() };
        self.message = Some(match std::fs::write(&path, preset.to_json()) {
            Ok(()) => (format!("Saved {}", file_name(&path)), false),
            Err(e) => (format!("Couldn't save {}: {e}", file_name(&path)), true),
        });
    }
}

enum ArcAction {
    Swap(usize, usize),
    Remove(usize),
    Add,
    Import,
    Export,
}

/// A text box for a list like "1-6, 8", outlined in red if it isn't one.
fn number_list_editor(ui: &mut Ui, text: &mut String, width: f32) {
    let response = ui.add(egui::TextEdit::singleline(text).desired_width(width));
    if story_arcs::parse_numbers(text).is_none() {
        let stroke = Stroke::new(1.5, Color32::from_rgb(220, 70, 70));
        ui.painter().rect_stroke(response.rect, 2.0, stroke, StrokeKind::Outside);
        response.on_hover_text("Must be numbers or ranges, like 1-6, 8");
    }
}

/// "12 volumes, 40 chapters".
fn file_counts(series: &Series) -> String {
    let volumes = series.files.iter().filter(|f| f.place.chapter.is_none() && f.place.volume.is_some()).count();
    let chapters = series.files.iter().filter(|f| f.place.chapter.is_some()).count();
    let other = series.files.len() - volumes - chapters;
    let mut parts = Vec::new();
    for (count, one, many) in [(volumes, "volume", "volumes"), (chapters, "chapter", "chapters"), (other, "other file", "other files")] {
        match count {
            0 => {}
            1 => parts.push(format!("1 {one}")),
            n => parts.push(format!("{n} {many}")),
        }
    }
    parts.join(", ")
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::story_arcs::{File, Place};

    fn file(name: &str, volume: u32, arc: &str, part: &str) -> File {
        File {
            path: PathBuf::from(name),
            file_name: name.into(),
            place: Place { volume: Some(volume), chapter: None },
            story_arc: arc.into(),
            story_arc_number: part.into(),
            writable: true,
        }
    }

    #[test]
    fn applies_only_what_changes() {
        let files = vec![
            file("v01", 1, "Black Swordsman", "1"),
            file("v02", 2, "", ""),
            file("v03", 3, "Golden Age", "1"),
        ];
        let mut state = SeriesState::new(Series { name: "Berserk".into(), files });
        assert!(state.edits().unwrap().is_empty(), "the arcs read from the tags change nothing");

        state.arcs[0].volumes = "1-2".into();
        state.arcs.remove(1);
        let edits = state.edits().unwrap();
        let expected: Vec<(PathBuf, FieldValues)> = vec![
            ("v02".into(), vec![("StoryArc", "Black Swordsman".into()), ("StoryArcNumber", "2".into())]),
            ("v03".into(), vec![("StoryArc", String::new()), ("StoryArcNumber", String::new())]),
        ];
        assert_eq!(edits, expected, "a removed arc's files are untagged");
    }

    #[test]
    fn selection_becomes_an_arc() {
        let files = (1..=6).map(|v| file(&format!("v{v:02}"), v, "", "")).collect();
        let mut state = SeriesState::new(Series { name: "Berserk".into(), files });
        state.arcs.push(Arc { name: "Everything".into(), volumes: "1-6".into(), chapters: String::new() });
        state.click(2, egui::Modifiers::NONE);
        state.click(4, egui::Modifiers::SHIFT);
        state.arc_from_selection();
        let ranges: Vec<(&str, &str)> = state.arcs.iter().map(|a| (a.name.as_str(), a.volumes.as_str())).collect();
        assert_eq!(ranges, [("Everything", "1-2, 6"), ("Arc 2", "3-5")]);
        assert!(state.selected.is_empty());
    }
}
