//! Background jobs that process a batch of comics one at a time (saving
//! metadata, converting, compressing) and keep the library database in sync.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use anyhow::Result;

use crate::library::{ComicRecord, Database};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    SaveInfo,
    ConvertToCbz,
    CompressAvif,
}

impl JobKind {
    pub fn title(self) -> &'static str {
        match self {
            JobKind::SaveInfo => "Saving metadata",
            JobKind::ConvertToCbz => "Converting to CBZ",
            JobKind::CompressAvif => "Compressing to AVIF",
        }
    }
}

pub enum JobEvent {
    Progress { done: usize, total: usize, current: String },
    Finished(Result<JobReport>),
}

#[derive(Debug, Default)]
pub struct JobReport {
    pub succeeded: usize,
    pub errors: Vec<(PathBuf, String)>,
    pub cancelled: bool,
    /// Total size of the successfully processed comics before and after.
    pub bytes_before: u64,
    pub bytes_after: u64,
}

pub struct JobHandle {
    pub kind: JobKind,
    pub events: Receiver<JobEvent>,
    cancel: Arc<AtomicBool>,
}

impl JobHandle {
    /// Stops the job after the comic currently being processed.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelling(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Runs `work` for each path on a background thread.
///
/// `work` returns the comic as it now exists on disk. If its path changed
/// (e.g. `.cbr` → `.cbz`), the old library entry is replaced. `notify` is
/// called after each event so the UI can repaint.
pub fn spawn<W, N>(kind: JobKind, db_path: PathBuf, paths: Vec<PathBuf>, work: W, notify: N) -> JobHandle
where
    W: Fn(&Path) -> Result<ComicRecord> + Send + 'static,
    N: Fn() + Send + 'static,
{
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::clone(&cancel);

    thread::spawn(move || {
        let send = |event| {
            let _ = tx.send(event);
            notify();
        };
        let progress = |done, current| send(JobEvent::Progress { done, total: paths.len(), current });
        let result = run(kind, &db_path, &paths, &work, &cancelled, &progress);
        send(JobEvent::Finished(result));
    });

    JobHandle { kind, events: rx, cancel }
}

fn run(
    kind: JobKind,
    db_path: &Path,
    paths: &[PathBuf],
    work: &dyn Fn(&Path) -> Result<ComicRecord>,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize, String),
) -> Result<JobReport> {
    let mut db = Database::open(db_path)?;
    let mut report = JobReport::default();

    for (i, path) in paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        progress(i, path.file_name().unwrap_or_default().to_string_lossy().into_owned());

        let size_before = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let result = work(path).and_then(|mut record| {
            let size_after = record.size;
            let mut removed = Vec::new();
            if record.path != *path {
                // Still the same comic: keep when it was added.
                record.added = db.added(path)?.unwrap_or(record.added);
                db.move_comic_data(path, &record.path)?;
                removed.push(path.clone());
            }
            if kind == JobKind::CompressAvif && size_before > 0 {
                db.record_original_size(&record.path, size_before)?;
            }
            db.apply_changes(&[record], &removed)?;
            Ok(size_after)
        });
        match result {
            Ok(size_after) => {
                report.succeeded += 1;
                report.bytes_before += size_before;
                report.bytes_after += size_after;
            }
            Err(e) => report.errors.push((path.clone(), format!("{e:#}"))),
        }
    }
    progress(paths.len(), String::new());
    Ok(report)
}
