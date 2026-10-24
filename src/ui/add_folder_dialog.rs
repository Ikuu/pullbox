//! Asks what a newly picked library folder holds.

use std::path::PathBuf;

use egui::RichText;

use crate::library::LibraryKind;

pub enum AddFolderOutcome {
    Add(PathBuf, LibraryKind),
    Cancel,
}

pub struct AddFolderDialog {
    path: PathBuf,
    kind: LibraryKind,
}

impl AddFolderDialog {
    pub fn new(path: PathBuf) -> Self {
        let kind = LibraryKind::guess(&path);
        Self { path, kind }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<AddFolderOutcome> {
        let modal = egui::Modal::new(egui::Id::new("add_folder_dialog")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Add Library Folder");
            ui.label(RichText::new(self.path.display().to_string()).weak());
            ui.separator();

            ui.strong("This folder holds");
            for kind in LibraryKind::ALL {
                ui.radio_value(&mut self.kind, kind, kind.label());
                ui.indent(kind.key(), |ui| ui.label(RichText::new(describe(kind)).small().weak()));
            }
            ui.add_space(4.0);
            ui.label(RichText::new("You can change this later by right-clicking the folder.").small().weak());
            ui.separator();

            let mut outcome = None;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Add").clicked() {
                    outcome = Some(AddFolderOutcome::Add(self.path.clone(), self.kind));
                }
                if ui.button("Cancel").clicked() {
                    outcome = Some(AddFolderOutcome::Cancel);
                }
            });
            outcome
        });

        if modal.should_close() {
            return Some(AddFolderOutcome::Cancel);
        }
        modal.inner
    }
}

fn describe(kind: LibraryKind) -> &'static str {
    match kind {
        LibraryKind::Comic => "Looked up on ComicVine.",
        LibraryKind::Manga => "Tagged as manga, read right to left.",
    }
}
