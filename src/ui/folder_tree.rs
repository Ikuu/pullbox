//! Left-hand library tree: root folders and the subfolders that contain comics.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use egui::collapsing_header::CollapsingState;

use crate::library::ComicRecord;
use crate::util::natural_cmp;

/// The part of the library the comic pane shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    /// Comics added in the last few days.
    Recent,
    /// A folder, with or without its subfolders.
    Folder(PathBuf),
}

impl Scope {
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Scope::Folder(folder) => Some(folder),
            _ => None,
        }
    }
}

pub enum TreeAction {
    Select(Scope),
    /// Re-read a folder from disk; `None` refreshes the whole library.
    Refresh(Option<PathBuf>),
    RemoveRoot(PathBuf),
    OpenInFileManager(PathBuf),
}

#[derive(Default)]
pub struct FolderTree {
    roots: Vec<FolderNode>,
    total: usize,
    /// Comics added since the "Recently Added" cutoff.
    recent: usize,
}

struct FolderNode {
    path: PathBuf,
    name: String,
    /// Comics in this folder and all of its subfolders.
    comic_count: usize,
    children: Vec<FolderNode>,
}

impl FolderTree {
    /// `recent_since` is the "Recently Added" cutoff, in seconds since the Unix epoch.
    pub fn build(roots: &[PathBuf], comics: &[ComicRecord], recent_since: i64) -> Self {
        let mut direct_counts: BTreeMap<&Path, usize> = BTreeMap::new();
        for comic in comics {
            *direct_counts.entry(comic.folder()).or_default() += 1;
        }

        let roots = roots
            .iter()
            .map(|root| {
                // Every folder between the root and a folder holding comics.
                let mut folders = BTreeSet::new();
                for dir in direct_counts.keys().filter(|d| d.starts_with(root)) {
                    for ancestor in dir.ancestors().take_while(|a| a.starts_with(root)) {
                        folders.insert(ancestor);
                    }
                }
                let mut node = build_node(root, &folders, &direct_counts);
                node.name = root.display().to_string();
                node
            })
            .collect();

        let recent = comics.iter().filter(|c| c.added >= recent_since).count();
        Self { roots, total: comics.len(), recent }
    }

    /// Draws the tree. A non-empty `filter` shows only folders whose name
    /// contains it (case-insensitive), plus the folders leading to them.
    pub fn show(&self, ui: &mut egui::Ui, scope: &Scope, filter: &str) -> Option<TreeAction> {
        let needle = filter.trim().to_lowercase();
        let filter = if needle.is_empty() { Filter::Off } else { Filter::Searching(&needle) };
        let mut action = None;

        let entries = [
            (Scope::All, format!("All Comics  ({})", self.total)),
            (Scope::Recent, format!("Recently Added  ({})", self.recent)),
        ];
        for (entry, text) in entries {
            let is_selected = *scope == entry;
            let mut response = ui.selectable_label(is_selected, text);
            if entry == Scope::Recent {
                response = response.on_hover_text("Newest first. Change how far back it looks in Settings.");
            }
            if response.clicked() {
                action = Some(click_action(is_selected, entry));
            }
            response.context_menu(|ui| {
                if ui.add(egui::Button::new("Refresh Library").shortcut_text("F5")).clicked() {
                    action = Some(TreeAction::Refresh(None));
                    ui.close();
                }
            });
        }
        ui.separator();

        let selected = scope.folder();

        let mut any_shown = false;
        for root in &self.roots {
            any_shown |= show_node(ui, root, true, selected, filter, &mut action);
        }
        if !any_shown && filter != Filter::Off {
            ui.weak("No matching folders");
        }
        action
    }
}

/// Clicking an entry selects it; clicking the already-selected entry refreshes
/// its folder (or the whole library).
fn click_action(already_selected: bool, scope: Scope) -> TreeAction {
    if already_selected { TreeAction::Refresh(scope.folder().map(Path::to_path_buf)) } else { TreeAction::Select(scope) }
}

fn build_node(path: &Path, folders: &BTreeSet<&Path>, direct_counts: &BTreeMap<&Path, usize>) -> FolderNode {
    let mut children: Vec<FolderNode> = folders
        .iter()
        .filter(|f| f.parent() == Some(path))
        .map(|child| build_node(child, folders, direct_counts))
        .collect();
    children.sort_by(|a, b| natural_cmp(&a.name, &b.name));

    let own = direct_counts.get(path).copied().unwrap_or(0);
    FolderNode {
        path: path.to_path_buf(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        comic_count: own + children.iter().map(|c| c.comic_count).sum::<usize>(),
        children,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter<'a> {
    Off,
    /// Inside a folder that matched: everything is shown.
    InsideMatch,
    /// Looking for folders whose (lower-cased) name contains this.
    Searching(&'a str),
}

fn name_matches(node: &FolderNode, needle: &str) -> bool {
    node.name.to_lowercase().contains(needle)
}

fn subtree_matches(node: &FolderNode, needle: &str) -> bool {
    name_matches(node, needle) || node.children.iter().any(|c| subtree_matches(c, needle))
}

/// Draws `node` and its children. Returns false if the filter hid it.
fn show_node(
    ui: &mut egui::Ui,
    node: &FolderNode,
    is_root: bool,
    selected: Option<&Path>,
    filter: Filter,
    action: &mut Option<TreeAction>,
) -> bool {
    // Roots are full paths, so they're never matched by name themselves.
    let (child_filter, open_by_default) = match filter {
        Filter::Off => (Filter::Off, is_root),
        Filter::InsideMatch => (Filter::InsideMatch, false),
        Filter::Searching(needle) if !is_root && name_matches(node, needle) => (Filter::InsideMatch, false),
        // Expanded, as the path leading to a match further down.
        Filter::Searching(needle) if node.children.iter().any(|c| subtree_matches(c, needle)) => (filter, true),
        Filter::Searching(_) => return false,
    };

    let mut label = |ui: &mut egui::Ui| {
        let is_selected = selected == Some(node.path.as_path());
        let text = format!("{}  ({})", node.name, node.comic_count);
        let response = ui.selectable_label(is_selected, text).on_hover_text(node.path.display().to_string());
        if response.clicked() {
            *action = Some(click_action(is_selected, Scope::Folder(node.path.clone())));
        }
        response.context_menu(|ui| {
            if ui.button("Refresh").clicked() {
                *action = Some(TreeAction::Refresh(Some(node.path.clone())));
                ui.close();
            }
            if ui.button("Open in File Manager").clicked() {
                *action = Some(TreeAction::OpenInFileManager(node.path.clone()));
                ui.close();
            }
            if is_root {
                ui.separator();
                if ui.button("Remove from Library").clicked() {
                    *action = Some(TreeAction::RemoveRoot(node.path.clone()));
                    ui.close();
                }
            }
        });
    };

    if node.children.is_empty() {
        ui.horizontal(|ui| {
            // Line leaves up with siblings that have an expand arrow.
            ui.add_space(ui.spacing().icon_width + ui.spacing().item_spacing.x);
            label(ui);
        });
        return true;
    }

    // Separate expanded/collapsed state while filtering, so clearing the
    // filter restores the tree as it was.
    let id = ui.make_persistent_id((&node.path, filter != Filter::Off));
    CollapsingState::load_with_default_open(ui.ctx(), id, open_by_default)
        .show_header(ui, label)
        .body(|ui| {
            for child in &node.children {
                show_node(ui, child, false, selected, child_filter, action);
            }
        });
    true
}
