//! Multi-selection of comics, shared by the list and grid views.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::library::ComicRecord;

#[derive(Default)]
pub struct Selection {
    /// Keyed by path so selection survives re-sorting and reloads.
    paths: HashSet<PathBuf>,
    /// Position (in the visible list) that shift-click ranges extend from.
    anchor: Option<usize>,
}

impl Selection {
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    /// Selected comics in display order.
    pub fn paths_in_order(&self, comics: &[ComicRecord], visible: &[usize]) -> Vec<PathBuf> {
        visible
            .iter()
            .map(|&i| &comics[i].path)
            .filter(|path| self.paths.contains(*path))
            .cloned()
            .collect()
    }

    pub fn select_all(&mut self, comics: &[ComicRecord], visible: &[usize]) {
        self.paths = visible.iter().map(|&i| comics[i].path.clone()).collect();
    }

    pub fn select_only(&mut self, position: usize, path: &Path) {
        self.paths.clear();
        self.paths.insert(path.to_path_buf());
        self.anchor = Some(position);
    }

    /// Applies a click on the item at `position` in the visible list,
    /// honouring Ctrl/Cmd (toggle) and Shift (range).
    pub fn click(&mut self, modifiers: egui::Modifiers, position: usize, comics: &[ComicRecord], visible: &[usize]) {
        let path = &comics[visible[position]].path;
        if modifiers.shift {
            let anchor = self.anchor.unwrap_or(position).min(visible.len().saturating_sub(1));
            let (start, end) = (anchor.min(position), anchor.max(position));
            if !modifiers.command {
                self.paths.clear();
            }
            self.paths
                .extend(visible[start..=end].iter().map(|&i| comics[i].path.clone()));
        } else if modifiers.command {
            if !self.paths.remove(path) {
                self.paths.insert(path.clone());
            }
            self.anchor = Some(position);
        } else {
            self.select_only(position, path);
        }
    }
}
