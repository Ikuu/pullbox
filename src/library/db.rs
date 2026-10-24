//! SQLite cache of library folders and the comics found in them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::model::{ComicMeta, ComicRecord, LibraryKind, Root};
use crate::archive::Format;

/// Tables of extra facts about comics, keyed by path. Their rows follow a
/// comic when it's renamed and go when it leaves the library.
const COMIC_DATA_TABLES: [&str; 2] = ["collects", "original_sizes"];

/// Bump when the schema changes and add a step to `migrate`.
const SCHEMA_VERSION: i32 = 7;

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
            // The column may already exist: builds before 0.1.0 reset
            // `user_version` to 2 when they opened an upgraded database.
            if !self.has_column("comics", "added")? {
                self.conn
                    .execute_batch("ALTER TABLE comics ADD COLUMN added INTEGER NOT NULL DEFAULT 0;")?;
            }
            // The real date is unknown for comics already in the library; the
            // file's mtime is the closest guess. Zero (unscanned) is filled in
            // by the next scan.
            self.conn.execute_batch("UPDATE comics SET added = modified WHERE added = 0;")?;
        }
        if version < 4 {
            // Which issues a collected edition contains, as the user entered
            // them (e.g. "1-6"). Kept apart from `comics` so a rescan, which
            // replaces comic rows, doesn't lose them.
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS collects (
                     path   TEXT PRIMARY KEY,
                     issues TEXT NOT NULL
                 );",
            )?;
        }
        if version < 5 {
            // A comic's size before it was first compressed, for the space
            // saved. Comics compressed before this table existed have none.
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS original_sizes (
                     path TEXT PRIMARY KEY,
                     size INTEGER NOT NULL
                 );",
            )?;
        }
        if version < 6 && !self.has_column("roots", "kind")? {
            // What each library folder holds; existing folders hold comics.
            self.conn
                .execute_batch("ALTER TABLE roots ADD COLUMN kind TEXT NOT NULL DEFAULT 'comic';")?;
        }
        if version < 7 && !self.has_column("comics", "story_arc")? {
            // Clearing `modified` makes the next scan re-read every comic,
            // filling in the new columns.
            self.conn.execute_batch(
                "ALTER TABLE comics ADD COLUMN story_arc TEXT NOT NULL DEFAULT '';
                 ALTER TABLE comics ADD COLUMN story_arc_number TEXT NOT NULL DEFAULT '';
                 UPDATE comics SET modified = 0;",
            )?;
        }
        // Never lower the version: a newer build may have upgraded the database.
        if version < SCHEMA_VERSION {
            self.conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare("SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2")?;
        Ok(stmt.exists([table, column])?)
    }

    // ----- roots -----

    pub fn roots(&self) -> Result<Vec<Root>> {
        let mut stmt = self.conn.prepare("SELECT path, kind FROM roots ORDER BY path")?;
        let roots = stmt
            .query_map([], |r| {
                Ok(Root {
                    path: PathBuf::from(r.get::<_, String>(0)?),
                    kind: LibraryKind::from_key(&r.get::<_, String>(1)?),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(roots)
    }

    /// Adds a library folder, or changes the kind of one already added.
    pub fn add_root(&self, path: &Path, kind: LibraryKind) -> Result<()> {
        self.conn.execute(
            "INSERT INTO roots (path, kind) VALUES (?1, ?2)
             ON CONFLICT (path) DO UPDATE SET kind = excluded.kind",
            [path_str(path)?, kind.key()],
        )?;
        Ok(())
    }

    /// Removes a root folder and every cached comic beneath it.
    pub fn remove_root(&self, path: &Path) -> Result<()> {
        let path = path_str(path)?;
        self.conn.execute("DELETE FROM roots WHERE path = ?1", [path])?;
        self.conn
            .execute("DELETE FROM comics WHERE path LIKE ?1 ESCAPE '\\'", [prefix_pattern(path)])?;
        for table in COMIC_DATA_TABLES {
            self.conn
                .execute(&format!("DELETE FROM {table} WHERE path LIKE ?1 ESCAPE '\\'"), [prefix_pattern(path)])?;
        }
        Ok(())
    }

    // ----- collected editions -----

    /// The issues entered for collected editions, by path.
    pub fn all_collects(&self) -> Result<HashMap<PathBuf, String>> {
        let mut stmt = self.conn.prepare("SELECT path, issues FROM collects")?;
        let collects = stmt
            .query_map([], |r| Ok((PathBuf::from(r.get::<_, String>(0)?), r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(collects)
    }

    /// Records which issues the comic at `path` collects; empty forgets it.
    pub fn set_collects(&self, path: &Path, issues: &str) -> Result<()> {
        let path = path_str(path)?;
        if issues.trim().is_empty() {
            self.conn.execute("DELETE FROM collects WHERE path = ?1", [path])?;
        } else {
            self.conn
                .execute("INSERT OR REPLACE INTO collects (path, issues) VALUES (?1, ?2)", [path, issues.trim()])?;
        }
        Ok(())
    }

    /// Keeps what's recorded about a comic (collected issues, original
    /// size) when its file is renamed, e.g. `.cbr` → `.cbz`.
    pub fn move_comic_data(&self, from: &Path, to: &Path) -> Result<()> {
        for table in COMIC_DATA_TABLES {
            self.conn.execute(
                &format!("UPDATE OR REPLACE {table} SET path = ?2 WHERE path = ?1"),
                [path_str(from)?, path_str(to)?],
            )?;
        }
        Ok(())
    }

    // ----- compression savings -----

    /// Sizes before compression, by path.
    pub fn original_sizes(&self) -> Result<HashMap<PathBuf, u64>> {
        let mut stmt = self.conn.prepare("SELECT path, size FROM original_sizes")?;
        let sizes = stmt
            .query_map([], |r| Ok((PathBuf::from(r.get::<_, String>(0)?), r.get::<_, i64>(1)? as u64)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(sizes)
    }

    /// Records the size of a comic about to be compressed. Compressing it
    /// again keeps the larger, earlier size.
    pub fn record_original_size(&self, path: &Path, size: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO original_sizes (path, size) VALUES (?1, ?2)
             ON CONFLICT (path) DO UPDATE SET size = MAX(size, excluded.size)",
            params![path_str(path)?, size as i64],
        )?;
        Ok(())
    }

    // ----- comics -----

    pub fn all_comics(&self) -> Result<Vec<ComicRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, size, modified, format, page_count, has_info, series, number,
                    volume, title, year, month, publisher, writer, compressed, added,
                    story_arc, story_arc_number
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
                      volume, title, year, month, publisher, writer, compressed, added,
                      story_arc, story_arc_number)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         COALESCE((SELECT NULLIF(added, 0) FROM comics WHERE path = ?1), ?16),
                         ?17, ?18)",
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
                    m.story_arc,
                    m.story_arc_number,
                ])?;
            }

            let mut delete = tx.prepare("DELETE FROM comics WHERE path = ?1")?;
            for path in removed {
                delete.execute([path_str(path)?])?;
                for table in COMIC_DATA_TABLES {
                    tx.execute(&format!("DELETE FROM {table} WHERE path = ?1"), [path_str(path)?])?;
                }
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
            story_arc: r.get(16)?,
            story_arc_number: r.get(17)?,
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
    fn reopens_a_database_whose_version_was_lowered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        Database::open(&path).unwrap().apply_changes(&[record("/lib/a.cbz", 100)], &[]).unwrap();
        // What an older build does to an upgraded database.
        Connection::open(&path).unwrap().pragma_update(None, "user_version", 2).unwrap();

        let db = Database::open(&path).unwrap();
        assert_eq!(db.added(Path::new("/lib/a.cbz")).unwrap(), Some(100));
    }

    #[test]
    fn roots_remember_their_kind() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("library.db")).unwrap();
        db.add_root(Path::new("/lib/comics"), LibraryKind::Comic).unwrap();
        db.add_root(Path::new("/lib/manga"), LibraryKind::Comic).unwrap();
        db.add_root(Path::new("/lib/manga"), LibraryKind::Manga).unwrap();
        let kinds: Vec<LibraryKind> = db.roots().unwrap().into_iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [LibraryKind::Comic, LibraryKind::Manga]);
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

    #[test]
    fn collected_issues_follow_renames_and_go_with_removals() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(&dir.path().join("library.db")).unwrap();
        let (cbr, cbz) = (Path::new("/lib/v1.cbr"), Path::new("/lib/v1.cbz"));
        db.set_collects(cbr, " 1-6 ").unwrap();
        db.move_comic_data(cbr, cbz).unwrap();
        assert_eq!(db.all_collects().unwrap().get(cbz).map(String::as_str), Some("1-6"));

        db.apply_changes(&[], &[cbz.to_path_buf()]).unwrap();
        assert!(db.all_collects().unwrap().is_empty());

        db.set_collects(cbz, "1-6").unwrap();
        db.set_collects(cbz, "").unwrap();
        assert!(db.all_collects().unwrap().is_empty());
    }

    #[test]
    fn original_sizes_keep_the_first_compression() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(&dir.path().join("library.db")).unwrap();
        let (cbr, cbz) = (Path::new("/lib/a.cbr"), Path::new("/lib/a.cbz"));
        db.record_original_size(cbr, 100).unwrap();
        db.move_comic_data(cbr, cbz).unwrap();
        db.record_original_size(cbz, 60).unwrap();
        assert_eq!(db.original_sizes().unwrap().get(cbz), Some(&100));

        db.apply_changes(&[], &[cbz.to_path_buf()]).unwrap();
        assert!(db.original_sizes().unwrap().is_empty());
    }
}
