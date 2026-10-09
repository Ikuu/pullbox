//! Background scanning of library folders into the database.
//!
//! Only new or changed files (by size and mtime) are opened, so rescans of an
//! unchanged library are close to instant.

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
        let mut known = db.stamps_under(root)?;
        for candidate in find_comics(root) {
            let unchanged = known.remove(&candidate.path) == Some((candidate.size, candidate.modified));
            if !unchanged {
                changed.push(candidate);
            }
        }
        // Whatever is left in `known` no longer exists on disk.
        removed.extend(known.into_keys());
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

fn find_comics(root: &Path) -> impl Iterator<Item = Candidate> {
    WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && archive::is_comic_file(entry.path()))
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let (size, modified) = file_stamp(&meta);
            Some(Candidate {
                path: entry.into_path(),
                size,
                modified,
                added: added_time(&meta),
            })
        })
}
