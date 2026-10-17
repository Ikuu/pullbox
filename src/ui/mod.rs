//! Reusable UI components.

pub mod comic_actions;
pub mod comic_grid;
pub mod comic_table;
pub mod comicvine_dialog;
pub mod compress_dialog;
pub mod errors_dialog;
pub mod folder_tree;
pub mod info_editor;
pub mod quit_dialog;
pub mod selection;
pub mod settings_dialog;

pub use comic_actions::{ComicAction, ComicView};
pub use comic_grid::ComicGrid;
pub use comic_table::ComicTable;
pub use compress_dialog::{CompressDialog, CompressOutcome};
pub use errors_dialog::ErrorsDialog;
pub use folder_tree::{FolderTree, Scope, TreeAction};
pub use info_editor::{EditorOutcome, InfoEditor};
pub use quit_dialog::{QuitOutcome, show_quit_dialog};
pub use selection::Selection;
pub use settings_dialog::{SettingsDialog, SettingsOutcome};
pub use comicvine_dialog::{ComicVineDialog, FieldValues, ScrapeOutcome, Target as ComicVineTarget};
