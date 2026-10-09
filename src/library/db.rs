//! SQLite cache of library folders and the comics found in them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::model::{ComicMeta, ComicRecord};
use crate::archive::Format;

/// Bump when the schema changes and add a step to `migrate`.
const SCHEMA_VERSION: i32 = 3;

pub struct Database {
    conn: Connection,
}

/// Size and mtime of a cached file, used to skip unchanged files on rescan.
pub type FileStamp = (u64, i64);

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open database {}", path.display()))?;
        // WAL lets the scanner thread write while the UI reads.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: i32 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                "CREATE TABLE roots (
                     path TEXT PRIMARY KEY
                 );
                 CREATE TABLE comics (
                     path       TEXT PRIMARY KEY,
                     size       INTEGER NOT NULL,
                     modified   INTEGER NOT NULL,
                     format     TEXT NOT NULL,
                     page_count INTEGER NOT NULL,
                     has_info   INTEGER NOT NULL,
                     series     TEXT NOT NULL,
                     number     TEXT NOT NULL,
                     volume     INTEGER,
                     title      TEXT NOT NULL,
                     year       INTEGER,
                     month      INTEGER,
                     publisher  TEXT NOT NULL,
                     writer     TEXT NOT NULL
                 );",
            )?;
        }
        if version < 2 {
            // Clearing `modified` makes the next scan re-read every comic,
            // filling in the new column.
            self.conn.execute_batch(
                "ALTER TABLE comics ADD COLUMN compressed INTEGER NOT NULL DEFAULT 0;
                 UPDATE comics SET modified = 0;",
            )?;
        }
        if version < 3 {
            // The real date is unknown for comics already in the library; the
            // file's mtime is the closest guess. Zero (unscanned) is filled in
            // by the next scan.
            self.conn.execute_batch(
                "ALTER TABLE comics ADD COLUMN added INTEGER NOT NULL DEFAULT 0;
                 UPDATE comics SET added = modified;",
            )?;
        }
        self.conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    // ----- roots -----

    pub fn roots(&self) -> Result<Vec<PathBuf>> {
        let mut stmt = self.conn.prepare("SELECT path FROM roots ORDER BY path")?;
        let roots = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|p| p.map(PathBuf::from))
            .collect::<rusqlite::Result<_>>()?;
        Ok(roots)
    }

    pub fn add_root(&self, path: &Path) -> Result<()> {
        self.conn
            .execute("INSERT OR IGNORE INTO roots (path) VALUES (?1)", [path_str(path)?])?;
        Ok(())
    }

    /// Removes a root folder and every cached comic beneath it.
    pub fn remove_root(&self, path: &Path) -> Result<()> {
        let path = path_str(path)?;
        self.conn.execute("DELETE FROM roots WHERE path = ?1", [path])?;
        self.conn
            .execute("DELETE FROM comics WHERE path LIKE ?1 ESCAPE '\\'", [prefix_pattern(path)])?;
        Ok(())
    }

    // ----- comics -----

    pub fn all_comics(&self) -> Result<Vec<ComicRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, size, modified, format, page_count, has_info, series, number,
                    volume, title, year, month, publisher, writer, compressed, added
             FROM comics",
        )?;
        let comics = stmt
            .query_map([], record_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(comics)
    }

    /// Size/mtime of every cached comic beneath `root`.
    pub fn stamps_under(&self, root: &Path) -> Result<HashMap<PathBuf, FileStamp>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, size, modified FROM comics WHERE path LIKE ?1 ESCAPE '\\'")?;
        let stamps = stmt
            .query_map([prefix_pattern(path_str(root)?)], |r| {
                Ok((
                    PathBuf::from(r.get::<_, String>(0)?),
                    (r.get::<_, i64>(1)? as u64, r.get(2)?),
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(stamps)
    }

    /// When the comic at `path` was added to the library, if it's known.
    pub fn added(&self, path: &Path) -> Result<Option<i64>> {
        let added = self
            .conn
            .query_row("SELECT added FROM comics WHERE path = ?1", [path_str(path)?], |r| r.get(0))
            .optional()?;
        Ok(added.filter(|&a| a > 0))
    }

    /// Inserts or updates `upserts` and deletes `removed` in one transaction.
    /// A comic already in the library keeps its original `added` time.
    pub fn apply_changes(&mut self, upserts: &[ComicRecord], removed: &[PathBuf]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut upsert = tx.prepare(
                "INSERT OR REPLACE INTO comics
                     (path, size, modified, format, page_count, has_info, series, number,
                      volume, title, year, month, publisher, writer, compressed, added)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         COALESCE((SELECT NULLIF(added, 0) FROM comics WHERE path = ?1), ?16))",
            )?;
            for c in upserts {
                let m = &c.meta;
                upsert.execute(params![
                    path_str(&c.path)?,
                    c.size as i64,
                    c.modified,
                    c.format.label(),
                    c.page_count,
                    m.has_info,
                    m.series,
                    m.number,
                    m.volume,
                    m.title,
                    m.year,
                    m.month,
                    m.publisher,
                    m.writer,
                    c.compressed,
                    c.added,
                ])?;
            }

            let mut delete = tx.prepare("DELETE FROM comics WHERE path = ?1")?;
            for path in removed {
                delete.execute([path_str(path)?])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn record_from_row(r: &Row) -> rusqlite::Result<ComicRecord> {
    let format: String = r.get(3)?;
    Ok(ComicRecord {
        path: PathBuf::from(r.get::<_, String>(0)?),
        size: r.get::<_, i64>(1)? as u64,
        modified: r.get(2)?,
        added: r.get(15)?,
        format: Format::from_label(&format).unwrap_or(Format::Cbz),
        page_count: r.get(4)?,
        compressed: r.get(14)?,
        meta: ComicMeta {
            has_info: r.get(5)?,
            series: r.get(6)?,
            number: r.get(7)?,
            volume: r.get(8)?,
            title: r.get(9)?,
            year: r.get(10)?,
            month: r.get(11)?,
            publisher: r.get(12)?,
            writer: r.get(13)?,
        },
    })
}

/// Paths are stored as UTF-8 text; non-UTF-8 paths are rejected.
fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("path is not valid UTF-8: {}", path.display()))
}

/// LIKE pattern matching everything strictly beneath `dir`.
fn prefix_pattern(dir: &str) -> String {
    let sep = std::path::MAIN_SEPARATOR;
    let prefix = format!("{}{sep}", dir.trim_end_matches(sep));
    let escaped = prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("{escaped}%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::Summary;

    fn record(path: &str, added: i64) -> ComicRecord {
        let summary = Summary { format: Format::Cbz, page_count: 1, recompressible_pages: 1, comic_info: None };
        ComicRecord::from_summary(PathBuf::from(path), 1, 1, added, summary)
    }

    #[test]
    fn rewriting_a_comic_keeps_its_added_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(&dir.path().join("library.db")).unwrap();
        db.apply_changes(&[record("/lib/a.cbz", 100)], &[]).unwrap();
        db.apply_changes(&[record("/lib/a.cbz", 500)], &[]).unwrap();
        assert_eq!(db.added(Path::new("/lib/a.cbz")).unwrap(), Some(100));
        assert_eq!(db.all_comics().unwrap()[0].added, 100);
    }
}
