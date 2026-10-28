//! The comic library: a cached index of comics found under user-chosen folders.

mod db;
mod model;
pub mod scanner;
pub mod series_check;
pub mod stats;
pub mod story_arcs;

pub use db::Database;
pub use model::{ComicRecord, LibraryKind, Root, kind_of};

use std::path::PathBuf;

/// Location of the library database (e.g. `~/.local/share/pullbox/library.db`).
/// Can be overridden with the `PULLBOX_DB` environment variable.
pub fn default_db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("PULLBOX_DB") {
        return PathBuf::from(path);
    }
    crate::util::project_dirs()
        .map(|dirs| dirs.data_dir().join("library.db"))
        .unwrap_or_else(|| PathBuf::from("library.db"))
}
