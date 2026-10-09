//! User settings, stored as JSON in the config directory
//! (e.g. `~/.config/comic-editor/settings.json`).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub comicvine_api_key: String,
    /// The view the comic pane opens in.
    pub default_view: ViewMode,
    /// How far back "Recently Added" looks.
    pub recent_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { comicvine_api_key: String::new(), default_view: ViewMode::default(), recent_days: 30 }
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
        fs::write(&path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

fn settings_path() -> Option<PathBuf> {
    crate::util::project_dirs().map(|dirs| dirs.config_dir().join("settings.json"))
}
