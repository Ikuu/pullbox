//! CBZ (zip) archives, read and written in-process.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::{Pages, cover_entry, is_comicinfo_entry};
use crate::comicinfo;

/// Returns the image counts and the raw ComicInfo.xml, if any.
pub(super) fn read_summary(path: &Path) -> Result<(Pages, Option<String>)> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file)).context("not a valid zip archive")?;

    let pages = Pages::count(archive.file_names());
    let info_entry = archive.file_names().find(|n| is_comicinfo_entry(n)).map(str::to_owned);

    let xml = match info_entry {
        Some(name) => {
            let mut xml = String::new();
            archive.by_name(&name)?.read_to_string(&mut xml)?;
            Some(xml)
        }
        None => None,
    };

    Ok((pages, xml))
}

pub fn read_cover(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut archive = ZipArchive::new(BufReader::new(File::open(path)?)).context("not a valid zip archive")?;
    let Some(name) = cover_entry(archive.file_names()).map(str::to_owned) else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    archive.by_name(&name)?.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// Extracts the whole archive into `dest`. Entries with unsafe paths
/// (absolute, or escaping via `..`) are rejected by the zip crate.
pub fn extract_all(path: &Path, dest: &Path) -> Result<()> {
    let mut archive = ZipArchive::new(BufReader::new(File::open(path)?)).context("not a valid zip archive")?;
    archive.extract(dest)?;
    Ok(())
}

/// Writes every file under `dir` into `out` as a new CBZ, sorted by path.
/// Entries are stored uncompressed: images are already compressed.
pub fn pack_directory(dir: &Path, out: &mut File) -> Result<()> {
    let mut files: Vec<_> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .collect();
    files.sort();

    let mut writer = ZipWriter::new(BufWriter::new(out));
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored);
    for file in &files {
        // Zip entry names always use forward slashes.
        let name = file.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
        let options = if is_comicinfo_entry(&name) {
            stored.compression_method(CompressionMethod::Deflated)
        } else {
            stored
        };
        writer.start_file(name, options)?;
        std::io::copy(&mut File::open(file)?, &mut writer)?;
    }
    writer.finish()?.flush()?;
    Ok(())
}

/// Rewrites the archive with a new ComicInfo.xml. Other entries are copied
/// without recompression, and the original is replaced atomically.
pub fn write_comic_info(path: &Path, xml: &str) -> Result<()> {
    let original = File::open(path)?;
    let permissions = original.metadata()?.permissions();
    let mut archive = ZipArchive::new(BufReader::new(original)).context("not a valid zip archive")?;

    let dir = path.parent().context("archive has no parent directory")?;
    let mut temp = tempfile::Builder::new().prefix(".comic-editor-").tempfile_in(dir)?;
    {
        let mut writer = ZipWriter::new(BufWriter::new(temp.as_file_mut()));
        for i in 0..archive.len() {
            let entry = archive.by_index_raw(i)?;
            if !is_comicinfo_entry(entry.name()) {
                writer.raw_copy_file(entry)?;
            }
        }
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file(comicinfo::FILE_NAME, options)?;
        writer.write_all(xml.as_bytes())?;
        writer.finish()?.flush()?;
    }

    temp.as_file().set_permissions(permissions)?;
    temp.persist(path)?;
    Ok(())
}

// Unix-only: the test checks file permission bits.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn make_cbz(path: &Path, info: Option<&str>) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for name in ["001.jpg", "002.jpg"] {
            zip.start_file(name, stored).unwrap();
            zip.write_all(name.as_bytes()).unwrap();
        }
        if let Some(info) = info {
            zip.start_file("comicinfo.xml", stored).unwrap();
            zip.write_all(info.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn write_replaces_comicinfo_and_keeps_pages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.cbz");
        make_cbz(&path, Some("<ComicInfo><Series>Old</Series></ComicInfo>"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_comic_info(&path, "<ComicInfo><Series>New</Series></ComicInfo>").unwrap();

        let (pages, xml) = read_summary(&path).unwrap();
        assert_eq!(pages, Pages { total: 2, recompressible: 2 });
        assert!(xml.unwrap().contains("New"));

        let mut archive = ZipArchive::new(File::open(&path).unwrap()).unwrap();
        let mut page = String::new();
        archive.by_name("002.jpg").unwrap().read_to_string(&mut page).unwrap();
        assert_eq!(page, "002.jpg");
        assert_eq!(archive.len(), 3, "old ComicInfo.xml must be replaced, not duplicated");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temp files left behind");
    }

    #[test]
    fn write_adds_comicinfo_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.cbz");
        make_cbz(&path, None);
        write_comic_info(&path, "<ComicInfo><Series>Added</Series></ComicInfo>").unwrap();
        assert!(read_summary(&path).unwrap().1.unwrap().contains("Added"));
    }
}
