//! Background scanning of library folders into the database.
//!
//! Only new or changed files (by size and mtime) are opened, so rescans of an
//! unchanged library are close to instant. Hidden files and folders are
//! skipped, which also keeps half-written temporary files out of the library.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use anyhow::Result;
use rayon::prelude::*;
use walkdir::WalkDir;

use super::db::Database;
use super::model::{ComicRecord, added_time, file_stamp};
use crate::archive;

pub enum ScanEvent {
    Progress { done: usize, total: usize },
    Finished(Result<ScanReport>),
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub updated: usize,
    pub removed: usize,
    pub errors: Vec<(PathBuf, String)>,
    /// Library folders that couldn't be read (e.g. an unmounted drive). Their
    /// comics are kept in the library until the folder is back.
    pub unavailable: Vec<PathBuf>,
}

/// A running scan. Drop the handle to stop listening; the scan itself runs to completion.
pub struct ScanHandle {
    pub events: Receiver<ScanEvent>,
}

/// Starts scanning `roots` on a background thread. `notify` is called whenever
/// an event is sent, so the UI can wake up and repaint.
pub fn start(
    db_path: PathBuf,
    roots: Vec<PathBuf>,
    notify: impl Fn() + Send + Sync + 'static,
) -> ScanHandle {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let send = |event| {
            let _ = tx.send(event);
            notify();
        };
        let result = scan(&db_path, &roots, &send);
        send(ScanEvent::Finished(result));
    });
    ScanHandle { events: rx }
}

/// A comic file found on disk.
struct Candidate {
    path: PathBuf,
    size: u64,
    modified: i64,
    added: i64,
}

fn scan(db_path: &Path, roots: &[PathBuf], send: &(dyn Fn(ScanEvent) + Sync)) -> Result<ScanReport> {
    let mut db = Database::open(db_path)?;
    let mut report = ScanReport::default();
    let mut changed = Vec::new();
    let mut removed = Vec::new();

    for root in roots {
        if std::fs::read_dir(root).is_err() {
            report.unavailable.push(root.clone());
            continue;
        }
        let mut known = db.stamps_under(root)?;
        let found = find_comics(root);
        for candidate in found.comics {
            let unchanged = known.remove(&candidate.path) == Some((candidate.size, candidate.modified));
            if !unchanged {
                changed.push(candidate);
            }
        }
        // Whatever is left in `known` no longer exists on disk, unless it's
        // in a folder that couldn't be read.
        removed.extend(
            known
                .into_keys()
                .filter(|path| !found.unreadable.iter().any(|dir| path.starts_with(dir))),
        );
    }

    let total = changed.len();
    let done = AtomicUsize::new(0);
    let results: Vec<_> = changed
        .into_par_iter()
        .map(|c| {
            let result = archive::read_summary(&c.path)
                .map(|summary| ComicRecord::from_summary(c.path.clone(), c.size, c.modified, c.added, summary))
                .map_err(|e| (c.path, format!("{e:#}")));
            let done = done.fetch_add(1, Ordering::Relaxed) + 1;
            if done.is_multiple_of(25) || done == total {
                send(ScanEvent::Progress { done, total });
            }
            result
        })
        .collect();

    let mut records = Vec::with_capacity(results.len());
    for result in results {
        match result {
            Ok(record) => records.push(record),
            Err(error) => report.errors.push(error),
        }
    }

    db.apply_changes(&records, &removed)?;
    report.updated = records.len();
    report.removed = removed.len();
    Ok(report)
}

/// What a walk of one library folder found.
struct Found {
    comics: Vec<Candidate>,
    /// Folders the walk couldn't read, so their contents are unknown.
    unreadable: Vec<PathBuf>,
}

fn find_comics(root: &Path) -> Found {
    let mut found = Found { comics: Vec::new(), unreadable: Vec::new() };
    let walk = WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        // The root itself may be hidden (e.g. `~/.comics`); only skip what's inside it.
        .filter_entry(|entry| entry.depth() == 0 || !is_hidden(entry.file_name()));
    for entry in walk {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                if let Some(path) = error.path() {
                    found.unreadable.push(path.to_path_buf());
                }
                continue;
            }
        };
        if !entry.file_type().is_file() || !archive::is_comic_file(entry.path()) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            found.unreadable.push(entry.into_path());
            continue;
        };
        let (size, modified) = file_stamp(&meta);
        found.comics.push(Candidate {
            path: entry.into_path(),
            size,
            modified,
            added: added_time(&meta),
        });
    }
    found
}

/// Dotfiles: hidden folders like `.Trash-1000`, macOS `._name.cbz` resource
/// forks, and this app's own temporary `.pullbox-*` files.
fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scan_paths(db_path: &Path, root: &Path) -> (ScanReport, Vec<PathBuf>) {
        let report = scan(db_path, &[root.to_path_buf()], &|_| {}).unwrap();
        let mut paths: Vec<_> = Database::open(db_path).unwrap().all_comics().unwrap().into_iter().map(|c| c.path).collect();
        paths.sort();
        (report, paths)
    }

    fn write_comic(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        zip.start_file("001.jpg", zip::write::SimpleFileOptions::default()).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn skips_hidden_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".library");
        write_comic(&root.join("a.cbz"));
        write_comic(&root.join(".pullbox-123.cbz"));
        write_comic(&root.join(".Trash-1000/b.cbz"));

        let (_, paths) = scan_paths(&dir.path().join("library.db"), &root);
        assert_eq!(paths, [root.join("a.cbz")]);
    }

    #[test]
    fn keeps_comics_of_an_unavailable_folder() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("library.db");
        let root = dir.path().join("mounted");
        write_comic(&root.join("a.cbz"));
        scan_paths(&db_path, &root);

        // The drive is unmounted.
        fs::rename(&root, dir.path().join("elsewhere")).unwrap();
        let (report, paths) = scan_paths(&db_path, &root);
        assert_eq!(report.unavailable, std::slice::from_ref(&root));
        assert_eq!(report.removed, 0);
        assert_eq!(paths, [root.join("a.cbz")]);
    }
}
