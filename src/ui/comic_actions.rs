//! Interaction shared by the list and grid views: clicking, the right-click
//! menu and keyboard shortcuts.

use std::path::PathBuf;

use super::Selection;
use crate::icons;
use crate::library::{ComicRecord, Root, kind_of};

/// Something the user asked to do with comics.
pub enum ComicAction {
    /// Edit the metadata of these comics (in display order).
    EditInfo(Vec<PathBuf>),
    ConvertToCbz(Vec<PathBuf>),
    CompressAvif(Vec<PathBuf>),
    /// Look up on ComicVine, or AniList for manga.
    LookUp(Vec<PathBuf>),
    OpenInFileManager(PathBuf),
}

/// The comics a view is showing, and the current selection among them.
pub struct ComicView<'a> {
    pub comics: &'a [ComicRecord],
    /// Indices into `comics`, in display order.
    pub visible: &'a [usize],
    pub selection: &'a mut Selection,
    /// The library folders, which decide where comics are looked up.
    pub roots: &'a [Root],
}

impl ComicView<'_> {
    fn selected(&self) -> Vec<PathBuf> {
        self.selection.paths_in_order(self.comics, self.visible)
    }

    /// Handles Ctrl+A (select all) and Ctrl+I (edit info).
    pub fn keyboard_shortcuts(&mut self, ui: &egui::Ui) -> Option<ComicAction> {
        if ui.ctx().egui_wants_keyboard_input() {
            return None;
        }
        let (select_all, edit) = ui.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::A),
                i.modifiers.command && i.key_pressed(egui::Key::I),
            )
        });
        if select_all {
            self.selection.select_all(self.comics, self.visible);
        }
        (edit && !self.selection.is_empty()).then(|| ComicAction::EditInfo(self.selected()))
    }

    /// Handles clicks and the context menu on the item at `position`.
    pub fn item_interaction(&mut self, response: &egui::Response, position: usize) -> Option<ComicAction> {
        let comic = &self.comics[self.visible[position]];
        let mut action = None;

        if response.clicked() {
            let modifiers = response.ctx.input(|i| i.modifiers);
            self.selection.click(modifiers, position, self.comics, self.visible);
        }
        if response.double_clicked() {
            self.selection.select_only(position, &comic.path);
            action = Some(ComicAction::EditInfo(vec![comic.path.clone()]));
        }
        if response.secondary_clicked() && !self.selection.contains(&comic.path) {
            self.selection.select_only(position, &comic.path);
        }

        response.context_menu(|ui| {
            action = self.context_menu(ui, comic);
            if action.is_some() {
                ui.close();
            }
        });
        action
    }

    fn context_menu(&self, ui: &mut egui::Ui, comic: &ComicRecord) -> Option<ComicAction> {
        let count = self.selection.len();
        let edit_label = if count > 1 { format!("Edit Info ({count} comics)…") } else { "Edit Info…".to_owned() };

        if ui.add(icons::button("document-edit", "", &edit_label).shortcut_text("Ctrl+I")).clicked() {
            return Some(ComicAction::EditInfo(self.selected()));
        }
        let source = kind_of(self.roots, &comic.path).lookup_source();
        if ui.add(icons::button("globe", "", &format!("Look Up on {source}…"))).clicked() {
            return Some(ComicAction::LookUp(self.selected()));
        }
        ui.separator();
        if ui.add(icons::button("convert", "", "Convert to CBZ")).clicked() {
            return Some(ComicAction::ConvertToCbz(self.selected()));
        }
        if ui.add(icons::button("archive-insert", "", "Compress to AVIF…")).clicked() {
            return Some(ComicAction::CompressAvif(self.selected()));
        }
        ui.separator();
        if ui.add(icons::button("folder-open", "", "Show in File Manager")).clicked() {
            return Some(ComicAction::OpenInFileManager(comic.folder().to_path_buf()));
        }
        None
    }
}
