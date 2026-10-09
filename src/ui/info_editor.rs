//! The "Edit Info" dialog, for one comic or many at once.
//!
//! When several comics are edited together, fields whose values differ are
//! shown blank as "(multiple values)" and only fields the user actually
//! changes are written back.

use std::path::PathBuf;

use egui::{Color32, RichText, Stroke, StrokeKind};

use crate::comicinfo::ComicInfo;
use crate::comicinfo::fields::{self, FIELDS, FieldDef, FieldKind};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Details,
    Plot,
    Other,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Details, Tab::Plot, Tab::Other];

    fn title(self) -> &'static str {
        match self {
            Tab::Details => "Details",
            Tab::Plot => "Plot & Notes",
            Tab::Other => "Characters & More",
        }
    }

    /// Rows of field names; fields in a row share its width.
    fn layout(self) -> &'static [&'static [&'static str]] {
        match self {
            Tab::Details => &[
                &["Series", "Number", "Count"],
                &["Title"],
                &["Volume", "Year", "Month", "Day"],
                &["AlternateSeries", "AlternateNumber", "AlternateCount"],
                &["StoryArc", "SeriesGroup"],
                &["Writer", "Penciller"],
                &["Inker", "Colorist"],
                &["Letterer", "CoverArtist"],
                &["Editor", "Translator"],
                &["Publisher", "Imprint"],
                &["Genre", "Format"],
                &["AgeRating", "LanguageISO", "Manga", "BlackAndWhite"],
                &["Tags"],
            ],
            Tab::Plot => &[&["Summary"], &["Notes"], &["Review"]],
            Tab::Other => &[
                &["Characters"],
                &["Teams"],
                &["MainCharacterOrTeam"],
                &["Locations"],
                &["Web"],
                &["ScanInformation", "CommunityRating"],
            ],
        }
    }
}

pub enum EditorOutcome {
    Save {
        targets: Vec<PathBuf>,
        /// `(element name, new value)`; an empty value clears the field.
        changes: Vec<(&'static str, String)>,
    },
    /// Look the comics up on ComicVine; the editor stays open underneath.
    LookUpComicVine,
    Cancel,
}

struct FieldState {
    def: &'static FieldDef,
    value: String,
    initial: String,
    /// The comics being edited disagree on this field.
    mixed: bool,
}

impl FieldState {
    fn is_changed(&self) -> bool {
        self.value.trim() != self.initial
    }

    fn is_valid(&self) -> bool {
        self.def.kind.is_valid(self.value.trim())
    }
}

pub struct InfoEditor {
    targets: Vec<PathBuf>,
    /// Why the comics can't be saved, if they can't.
    read_only_reason: Option<String>,
    fields: Vec<FieldState>,
    tab: Tab,
}

const DIALOG_WIDTH: f32 = 760.0;

impl InfoEditor {
    pub fn new(targets: Vec<PathBuf>, infos: &[ComicInfo], read_only_reason: Option<String>) -> Self {
        let fields = FIELDS
            .iter()
            .map(|def| {
                let mut values = infos.iter().map(|info| info.get(def.name).unwrap_or_default());
                let first = values.next().unwrap_or_default();
                let mixed = values.any(|v| v != first);
                let initial = if mixed { String::new() } else { first.to_owned() };
                FieldState { def, value: initial.clone(), initial, mixed }
            })
            .collect();

        Self { targets, read_only_reason, fields, tab: Tab::Details }
    }

    pub fn targets(&self) -> &[PathBuf] {
        &self.targets
    }

    /// The field's current (possibly edited) value.
    pub fn value(&self, name: &str) -> &str {
        fields::order_of(name).map_or("", |i| self.fields[i].value.trim())
    }

    /// Puts `values` into the form as unsaved changes.
    pub fn fill(&mut self, values: &[(&str, String)]) {
        for (name, value) in values {
            if let Some(i) = fields::order_of(name) {
                self.fields[i].value = value.clone();
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<EditorOutcome> {
        let modal = egui::Modal::new(egui::Id::new("info_editor")).show(ctx, |ui| {
            ui.set_width(DIALOG_WIDTH);
            self.header(ui);
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(ctx.content_rect().height() * 0.7)
                .show(ui, |ui| self.tab_contents(ui));
            ui.separator();
            self.footer(ui)
        });

        // Deliberately not closing on backdrop clicks, so a stray click can't lose edits.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return Some(EditorOutcome::Cancel);
        }
        modal.inner
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Edit Info");
            ui.label(RichText::new(self.subject()).weak());
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for tab in Tab::ALL {
                ui.selectable_value(&mut self.tab, tab, tab.title());
            }
        });
    }

    fn subject(&self) -> String {
        match self.targets.as_slice() {
            [single] => single.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            many => format!("{} comics", many.len()),
        }
    }

    fn tab_contents(&mut self, ui: &mut egui::Ui) {
        for row in self.tab.layout() {
            let indices: Vec<usize> = row.iter().filter_map(|name| fields::order_of(name)).collect();
            field_row(ui, &mut self.fields, &indices);
            ui.add_space(6.0);
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui) -> Option<EditorOutcome> {
        let changed = self.fields.iter().filter(|f| f.is_changed()).count();
        let all_valid = self.fields.iter().all(FieldState::is_valid);

        let mut outcome = None;
        ui.horizontal(|ui| {
            if let Some(reason) = &self.read_only_reason {
                ui.label(RichText::new(reason).color(ui.visuals().warn_fg_color));
            } else if !all_valid {
                ui.label(RichText::new("Some fields have invalid values").color(ui.visuals().error_fg_color));
            } else if changed > 0 {
                ui.label(format!("{changed} field(s) changed"));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_save = changed > 0 && all_valid && self.read_only_reason.is_none();
                if ui.add_enabled(can_save, egui::Button::new("Save")).clicked() {
                    outcome = Some(EditorOutcome::Save {
                        targets: self.targets.clone(),
                        changes: self.changes(),
                    });
                }
                if ui.button("Cancel").clicked() {
                    outcome = Some(EditorOutcome::Cancel);
                }
                let lookup = ui
                    .add_enabled(self.read_only_reason.is_none(), egui::Button::new("Look Up on ComicVine…"))
                    .on_hover_text(if self.targets.len() == 1 {
                        "Fill the form from ComicVine, then review and save"
                    } else {
                        "Match each comic on ComicVine and save directly (unsaved edits here are discarded)"
                    });
                if lookup.clicked() {
                    outcome = Some(EditorOutcome::LookUpComicVine);
                }
            });
        });
        outcome
    }

    fn changes(&self) -> Vec<(&'static str, String)> {
        self.fields
            .iter()
            .filter(|f| f.is_changed())
            .map(|f| (f.def.name, f.value.trim().to_owned()))
            .collect()
    }
}

/// Lays out the given fields side by side, sized by how much text they hold.
fn field_row(ui: &mut egui::Ui, fields: &mut [FieldState], indices: &[usize]) {
    let weight = |kind: FieldKind| match kind {
        FieldKind::Integer | FieldKind::Decimal => 1.0,
        FieldKind::Choice(_) => 2.0,
        FieldKind::Text | FieldKind::MultiLine => 4.0,
    };
    let total_weight: f32 = indices.iter().map(|&i| weight(fields[i].def.kind)).sum();
    let spacing = ui.spacing().item_spacing.x;
    let usable = ui.available_width() - spacing * (indices.len() as f32 - 1.0);

    ui.horizontal_top(|ui| {
        for &i in indices {
            let width = usable * weight(fields[i].def.kind) / total_weight;
            ui.vertical(|ui| {
                ui.set_width(width);
                field_editor(ui, &mut fields[i], width);
            });
        }
    });
}

fn field_editor(ui: &mut egui::Ui, field: &mut FieldState, width: f32) {
    let mut label = RichText::new(field.def.label).small();
    if field.is_changed() {
        label = label.color(ui.visuals().warn_fg_color).strong();
    }
    ui.label(label);

    let hint = if field.mixed { "(multiple values)" } else { "" };
    let response = match field.def.kind {
        FieldKind::Choice(options) => choice_editor(ui, field, options, hint, width),
        FieldKind::MultiLine => ui.add(
            egui::TextEdit::multiline(&mut field.value)
                .hint_text(hint)
                .desired_width(width)
                .desired_rows(9),
        ),
        _ => ui.add(
            egui::TextEdit::singleline(&mut field.value)
                .hint_text(hint)
                .desired_width(width),
        ),
    };

    if !field.is_valid() {
        let stroke = Stroke::new(1.5, Color32::from_rgb(220, 70, 70));
        ui.painter().rect_stroke(response.rect, 2.0, stroke, StrokeKind::Outside);
        response.on_hover_text("Must be a number");
    }
}

fn choice_editor(
    ui: &mut egui::Ui,
    field: &mut FieldState,
    options: &[&str],
    hint: &str,
    width: f32,
) -> egui::Response {
    let selected = match field.value.as_str() {
        "" if !hint.is_empty() => hint,
        "" => "—",
        value => value,
    };
    egui::ComboBox::from_id_salt(field.def.name)
        .width(width)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut field.value, String::new(), "—");
            for option in options {
                ui.selectable_value(&mut field.value, (*option).to_owned(), *option);
            }
        })
        .response
}
