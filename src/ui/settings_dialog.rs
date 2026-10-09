//! The Settings dialog.

use crate::comicvine::API_KEY_URL;
use crate::settings::{Settings, ViewMode};

pub enum SettingsOutcome {
    Save(Settings),
    Cancel,
}

pub struct SettingsDialog {
    settings: Settings,
    show_key: bool,
    /// Shown above the form, e.g. why the dialog was opened.
    message: Option<String>,
}

impl SettingsDialog {
    pub fn new(settings: Settings, message: Option<String>) -> Self {
        Self { settings, show_key: false, message }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<SettingsOutcome> {
        let modal = egui::Modal::new(egui::Id::new("settings_dialog")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading("Settings");
            if let Some(message) = &self.message {
                ui.colored_label(ui.visuals().warn_fg_color, message);
            }
            ui.separator();

            ui.strong("Default view");
            ui.horizontal(|ui| {
                for mode in [ViewMode::List, ViewMode::Grid] {
                    ui.radio_value(&mut self.settings.default_view, mode, mode.label());
                }
            });
            ui.add_space(8.0);

            ui.strong("Recently Added");
            ui.horizontal(|ui| {
                ui.label("Show comics added in the last");
                ui.add(egui::DragValue::new(&mut self.settings.recent_days).range(1..=3650));
                ui.label("days");
            });
            ui.add_space(8.0);

            ui.strong("ComicVine API key");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.comicvine_api_key)
                        .password(!self.show_key)
                        .desired_width(340.0),
                );
                ui.checkbox(&mut self.show_key, "Show");
            });
            ui.hyperlink_to("Get a free API key", API_KEY_URL);
            ui.separator();

            let mut outcome = None;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Save").clicked() {
                    self.settings.comicvine_api_key = self.settings.comicvine_api_key.trim().to_owned();
                    outcome = Some(SettingsOutcome::Save(self.settings.clone()));
                }
                if ui.button("Cancel").clicked() {
                    outcome = Some(SettingsOutcome::Cancel);
                }
            });
            outcome
        });

        if modal.should_close() {
            return Some(SettingsOutcome::Cancel);
        }
        modal.inner
    }
}
