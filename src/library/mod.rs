//! The comic library: a cached index of comics found under user-chosen folders.

mod db;
mod model;
pub mod scanner;

pub use db::Database;
pub use model::ComicRecord;

use std::path::PathBuf;

/// Location of the library database (e.g. `~/.local/share/comic-editor/library.db`).
/// Can be overridden with the `COMIC_EDITOR_DB` environment variable.
pub fn default_db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("COMIC_EDITOR_DB") {
        return PathBuf::from(path);
    }
    crate::util::project_dirs()
        .map(|dirs| dirs.data_dir().join("library.db"))
        .unwrap_or_else(|| PathBuf::from("library.db"))
}
