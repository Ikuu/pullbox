//! Lists what the last scan or job couldn't process, and why.

use std::path::PathBuf;

use egui::RichText;

pub struct ErrorsDialog {
    title: String,
    errors: Vec<(PathBuf, String)>,
}

impl ErrorsDialog {
    pub fn new(title: String, errors: Vec<(PathBuf, String)>) -> Self {
        Self { title, errors }
    }

    pub fn count(&self) -> usize {
        self.errors.len()
    }

    /// Returns true once the user closes the dialog.
    pub fn show(&self, ctx: &egui::Context) -> bool {
        let modal = egui::Modal::new(egui::Id::new("errors_dialog")).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.heading(&self.title);
            ui.separator();

            egui::ScrollArea::vertical().max_height(400.0).auto_shrink([false, true]).show(ui, |ui| {
                for (path, error) in &self.errors {
                    ui.label(RichText::new(path.display().to_string()).strong());
                    ui.label(RichText::new(error).color(ui.visuals().warn_fg_color));
                    ui.add_space(6.0);
                }
            });
            ui.separator();

            let mut close = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.button("Close").clicked();
                if ui.button("Copy").on_hover_text("Copy the list to the clipboard").clicked() {
                    ui.ctx().copy_text(self.to_text());
                }
            });
            close
        });
        modal.inner || modal.should_close()
    }

    fn to_text(&self) -> String {
        self.errors.iter().map(|(path, error)| format!("{}: {error}\n", path.display())).collect()
    }
}
