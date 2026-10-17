//! Creating cover thumbnails, with an on-disk cache.

use std::collections::HashSet;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, RgbaImage};

use crate::archive;
use crate::library::ComicRecord;

/// Thumbnails are scaled to fit this box, which is large enough for the
/// biggest grid size on a HiDPI screen.
const MAX_WIDTH: u32 = 256;
const MAX_HEIGHT: u32 = 384;
const JPEG_QUALITY: u8 = 85;

/// Identifies one version of a comic file. Size and mtime are included so a
/// compressed or edited comic gets a fresh thumbnail.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThumbKey {
    pub path: PathBuf,
    pub size: u64,
    pub modified: i64,
}

impl ThumbKey {
    pub fn for_comic(comic: &ComicRecord) -> Self {
        Self { path: comic.path.clone(), size: comic.size, modified: comic.modified }
    }

    fn cache_file(&self, cache_dir: &Path) -> PathBuf {
        // DefaultHasher may change between Rust releases; that only costs a
        // one-off regeneration of the cache.
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        cache_dir.join(format!("{:016x}.jpg", hasher.finish()))
    }
}

/// Returns the cached thumbnail, generating and caching it if needed.
pub fn load_or_generate(key: &ThumbKey, cache_dir: &Path) -> Result<RgbaImage> {
    let cache_file = key.cache_file(cache_dir);
    // A missing or corrupt cache file just means regenerating it.
    if let Ok(cached) = image::open(&cache_file) {
        return Ok(cached.to_rgba8());
    }

    let cover = archive::read_cover(&key.path)?.context("archive has no images")?;
    let thumbnail = image::load_from_memory(&cover)
        .context("unsupported cover image")?
        .thumbnail(MAX_WIDTH, MAX_HEIGHT);

    if let Err(e) = save(&thumbnail, &cache_file) {
        eprintln!("thumbnail cache: {}: {e:#}", cache_file.display());
    }
    Ok(thumbnail.to_rgba8())
}

/// Deletes cached thumbnails other than those for `keep`, i.e. ones for
/// comics that were since edited, compressed or removed. Returns how many
/// were deleted.
pub fn prune_cache(cache_dir: &Path, keep: &[ThumbKey]) -> Result<usize> {
    let keep: HashSet<PathBuf> = keep.iter().map(|key| key.cache_file(cache_dir)).collect();
    let entries = match fs::read_dir(cache_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    let mut deleted = 0;
    for entry in entries {
        let path = entry?.path();
        if is_cache_file(&path) && !keep.contains(&path) && fs::remove_file(&path).is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Whether `path` is named like a thumbnail, so nothing else in the folder is deleted.
fn is_cache_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    name.strip_suffix(".jpg")
        .is_some_and(|hash| hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn save(thumbnail: &DynamicImage, file: &Path) -> Result<()> {
    let mut jpeg = Vec::new();
    thumbnail
        .to_rgb8()
        .write_with_encoder(JpegEncoder::new_with_quality(Cursor::new(&mut jpeg), JPEG_QUALITY))?;
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(file, jpeg)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A CBZ whose cover is `01.<ext>`; AVIF covers are encoded with avifenc.
    fn comic_with_cover(dir: &Path, ext: &str) -> PathBuf {
        let pages = dir.join(format!("pages-{ext}"));
        fs::create_dir_all(&pages).unwrap();
        let jpg = pages.join("01.jpg");
        crate::util::write_test_page(&jpg, 800, 1200);
        if ext == "avif" {
            let avif = Command::new("avifenc").arg(&jpg).arg(pages.join("01.avif")).output();
            assert!(avif.unwrap().status.success(), "avifenc is needed for this test");
            fs::remove_file(&jpg).unwrap();
        }
        // A later page that must not be picked as the cover.
        fs::write(pages.join("10.jpg"), "not an image").unwrap();
        let cbz = dir.join(format!("{ext}.cbz"));
        archive::pack_directory(&pages, &mut fs::File::create(&cbz).unwrap()).unwrap();
        cbz
    }

    fn generates_and_caches_thumbnail(ext: &str) {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let path = comic_with_cover(dir.path(), ext);
        let key = ThumbKey { path, size: 1, modified: 1 };

        let thumb = load_or_generate(&key, &cache).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (MAX_WIDTH, MAX_HEIGHT));
        assert!(key.cache_file(&cache).exists(), "thumbnail should be cached");
    }

    #[test]
    fn prunes_thumbnails_of_changed_comics() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let path = comic_with_cover(dir.path(), "jpg");
        let old = ThumbKey { path: path.clone(), size: 1, modified: 1 };
        let new = ThumbKey { path, size: 2, modified: 2 };
        load_or_generate(&old, &cache).unwrap();
        load_or_generate(&new, &cache).unwrap();
        fs::write(cache.join("notes.txt"), "keep me").unwrap();

        assert_eq!(prune_cache(&cache, std::slice::from_ref(&new)).unwrap(), 1);
        assert!(!old.cache_file(&cache).exists());
        assert!(new.cache_file(&cache).exists());
        assert!(cache.join("notes.txt").exists());
    }

    #[test]
    fn generates_and_caches_jpeg_thumbnails() {
        generates_and_caches_thumbnail("jpg");
    }

    #[test]
    #[ignore = "needs avifenc; run with --ignored"]
    fn generates_and_caches_avif_thumbnails() {
        generates_and_caches_thumbnail("avif");
    }
}
