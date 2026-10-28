//! User settings, stored as JSON in the config directory
//! (e.g. `~/.config/pullbox/settings.json`).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub comicvine_api_key: String,
    /// Publishers whose series ComicVine lookups hide, e.g. foreign-language
    /// reprints.
    pub comicvine_ignored_publishers: Vec<String>,
    /// The view the comic pane opens in.
    pub default_view: ViewMode,
    /// How far back "Recently Added" looks.
    pub recent_days: u32,
    /// Whether the details pane is open.
    pub show_details: bool,
    /// Titles of the list columns the user has hidden.
    pub hidden_columns: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            comicvine_api_key: String::new(),
            comicvine_ignored_publishers: Vec::new(),
            default_view: ViewMode::default(),
            recent_days: 30,
            show_details: true,
            hidden_columns: Vec::new(),
        }
    }
}

/// How the comic pane shows comics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    #[default]
    List,
    Grid,
}

impl ViewMode {
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::List => "List",
            ViewMode::Grid => "Covers",
        }
    }
}

impl Settings {
    /// Loads settings, falling back to defaults if the file is missing or invalid.
    pub fn load() -> Self {
        let Some(path) = settings_path() else { return Self::default() };
        match fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
                eprintln!("ignoring invalid settings file {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path().context("no config directory available")?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        write_private(&path, serde_json::to_string_pretty(self)?.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

/// Writes `contents` to a file only the current user can read: it holds the
/// ComicVine API key.
fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        // `mode` only applies to new files; tighten one saved by older builds.
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
    }
    options.open(path)?.write_all(contents)
}

/// Where the settings are saved (e.g. `~/.config/pullbox/settings.json`).
pub fn settings_path() -> Option<PathBuf> {
    crate::util::project_dirs().map(|dirs| dirs.config_dir().join("settings.json"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn settings_file_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // As saved by older builds.
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        write_private(&path, b"{\"comicvine_api_key\": \"secret\"}").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(fs::read_to_string(&path).unwrap().contains("secret"));
    }
}
