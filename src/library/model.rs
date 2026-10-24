//! In-memory representation of a comic in the library.

use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::archive::{self, Format, Summary};
use crate::util::lower_ext;

/// What a library folder holds. Decides where its comics are looked up and
/// how they're tagged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryKind {
    #[default]
    Comic,
    Manga,
}

impl LibraryKind {
    pub const ALL: [LibraryKind; 2] = [LibraryKind::Comic, LibraryKind::Manga];

    pub fn label(self) -> &'static str {
        match self {
            LibraryKind::Comic => "Comics",
            LibraryKind::Manga => "Manga",
        }
    }

    /// How the kind is stored in the database.
    pub fn key(self) -> &'static str {
        match self {
            LibraryKind::Comic => "comic",
            LibraryKind::Manga => "manga",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "manga" => LibraryKind::Manga,
            _ => LibraryKind::Comic,
        }
    }

    /// Where comics of this kind are looked up.
    pub fn lookup_source(self) -> &'static str {
        match self {
            LibraryKind::Comic => "ComicVine",
            LibraryKind::Manga => "AniList",
        }
    }

    /// A first guess for a newly added folder, from its name.
    pub fn guess(folder: &Path) -> Self {
        let name = folder.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        if name.contains("manga") { LibraryKind::Manga } else { LibraryKind::Comic }
    }
}

/// A folder the user added to the library.
#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    pub path: PathBuf,
    pub kind: LibraryKind,
}

/// The kind of the library folder holding `path`. Roots can be nested; the
/// innermost wins.
pub fn kind_of(roots: &[Root], path: &Path) -> LibraryKind {
    roots
        .iter()
        .filter(|r| path.starts_with(&r.path))
        .max_by_key(|r| r.path.components().count())
        .map_or(LibraryKind::Comic, |r| r.kind)
}

/// One comic file plus the metadata fields shown in the library view.
#[derive(Clone, Debug)]
pub struct ComicRecord {
    pub path: PathBuf,
    pub size: u64,
    /// Modification time in seconds since the Unix epoch; used to detect changes.
    pub modified: i64,
    /// When the comic first appeared in the library, in seconds since the Unix
    /// epoch. Kept when the file is later rewritten (edited, compressed, …).
    pub added: i64,
    pub format: Format,
    pub page_count: u32,
    /// No pages left that AVIF compression would re-encode.
    pub compressed: bool,
    pub meta: ComicMeta,
}

/// The subset of ComicInfo fields indexed by the library.
#[derive(Clone, Debug, Default)]
pub struct ComicMeta {
    pub has_info: bool,
    pub series: String,
    pub number: String,
    pub volume: Option<i32>,
    pub title: String,
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub publisher: String,
    pub writer: String,
}

impl ComicRecord {
    /// Reads a comic's current state from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path)?;
        let (size, modified) = file_stamp(&meta);
        let summary = archive::read_summary(path)?;
        Ok(Self::from_summary(path.to_path_buf(), size, modified, added_time(&meta), summary))
    }

    pub fn from_summary(path: PathBuf, size: u64, modified: i64, added: i64, summary: Summary) -> Self {
        let meta = match &summary.comic_info {
            Some(info) => {
                let text = |name| info.get(name).unwrap_or_default().to_owned();
                ComicMeta {
                    has_info: true,
                    series: text("Series"),
                    number: text("Number"),
                    volume: info.get_i32("Volume"),
                    title: text("Title"),
                    year: info.get_i32("Year"),
                    month: info.get_i32("Month"),
                    publisher: text("Publisher"),
                    writer: text("Writer"),
                }
            }
            None => ComicMeta::default(),
        };

        Self {
            path,
            size,
            modified,
            added,
            format: summary.format,
            page_count: summary.page_count,
            compressed: summary.page_count > 0 && summary.recompressible_pages == 0,
            meta,
        }
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Whether converting to CBZ would change anything (RAR content or a
    /// non-`.cbz` extension).
    pub fn needs_cbz_conversion(&self) -> bool {
        self.format == Format::Cbr || lower_ext(&self.path) != "cbz"
    }

    /// Short name for captions: `Series #Number`, or the file name without extension.
    pub fn display_title(&self) -> String {
        let m = &self.meta;
        match (m.series.is_empty(), m.number.is_empty()) {
            (false, false) => format!("{} #{}", m.series, m.number),
            (false, true) => m.series.clone(),
            _ => self
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }

    pub fn folder(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new(""))
    }

    /// Case-insensitive match against the fields a user is likely to search.
    pub fn matches(&self, needle_lower: &str) -> bool {
        let m = &self.meta;
        [&self.file_name(), &m.series, &m.title, &m.publisher, &m.writer]
            .iter()
            .any(|field| field.to_lowercase().contains(needle_lower))
    }
}

/// Size and modification time (seconds since the Unix epoch) of a file.
pub fn file_stamp(meta: &Metadata) -> (u64, i64) {
    (meta.len(), epoch_secs(meta.modified()))
}

/// Best guess at when a file was put in the library: its creation time, or
/// its modification time where the filesystem doesn't record creation.
/// (Copies often keep the original mtime, so creation time is preferred.)
pub fn added_time(meta: &Metadata) -> i64 {
    match epoch_secs(meta.created()) {
        0 => epoch_secs(meta.modified()),
        created => created,
    }
}

fn epoch_secs(time: std::io::Result<SystemTime>) -> i64 {
    time.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_innermost_library_folder_decides_the_kind() {
        let root = |path: &str, kind| Root { path: PathBuf::from(path), kind };
        let roots = [root("/lib", LibraryKind::Comic), root("/lib/manga", LibraryKind::Manga)];
        assert_eq!(kind_of(&roots, Path::new("/lib/manga/Berserk/v01.cbz")), LibraryKind::Manga);
        assert_eq!(kind_of(&roots, Path::new("/lib/Saga/001.cbz")), LibraryKind::Comic);
        assert_eq!(kind_of(&roots, Path::new("/elsewhere/a.cbz")), LibraryKind::Comic);
        assert_eq!(LibraryKind::guess(Path::new("/media/Manga Collection")), LibraryKind::Manga);
    }
}
