//! Confirmation dialog for AVIF compression, with encoder settings.

use std::path::PathBuf;

use egui::RichText;

use crate::convert::CompressOptions;

pub enum CompressOutcome {
    Start { targets: Vec<PathBuf>, options: CompressOptions },
    Cancel,
}

pub struct CompressDialog {
    targets: Vec<PathBuf>,
    /// Selected comics left out because they're already compressed.
    already_compressed: usize,
    options: CompressOptions,
}

impl CompressDialog {
    pub fn new(targets: Vec<PathBuf>, already_compressed: usize, options: CompressOptions) -> Self {
        Self { targets, already_compressed, options }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<CompressOutcome> {
        let modal = egui::Modal::new(egui::Id::new("compress_dialog")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Compress to AVIF");
            ui.label(RichText::new(format!("{} comic(s) to compress", self.targets.len())).weak());
            if self.already_compressed > 0 {
                ui.label(
                    RichText::new(format!("{} already compressed — skipped", self.already_compressed))
                        .color(ui.visuals().warn_fg_color),
                );
            }
            ui.separator();

            ui.add_enabled(
                !self.options.lossless,
                egui::Slider::new(&mut self.options.quality, 0..=100).text("Quality"),
            );
            ui.checkbox(&mut self.options.lossless, "Lossless");
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "JPEG, PNG, BMP and TIFF pages are re-encoded with avifenc; GIF and WebP \
                     are kept. Pages that wouldn't get smaller are left as they are. \
                     Originals are moved to the Trash.",
                )
                .small()
                .weak(),
            );
            ui.separator();

            let mut outcome = None;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Compress").clicked() {
                    outcome = Some(CompressOutcome::Start {
                        targets: self.targets.clone(),
                        options: self.options,
                    });
                }
                if ui.button("Cancel").clicked() {
                    outcome = Some(CompressOutcome::Cancel);
                }
            });
            outcome
        });

        if modal.should_close() {
            return Some(CompressOutcome::Cancel);
        }
        modal.inner
    }
}
