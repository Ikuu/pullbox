//! Creating cover thumbnails, with an on-disk cache.

use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, RgbaImage};

use crate::archive;

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

    /// A CBZ whose cover is `01.<ext>`, produced with magick (and avifenc for AVIF).
    fn comic_with_cover(dir: &Path, ext: &str) -> PathBuf {
        let pages = dir.join(format!("pages-{ext}"));
        fs::create_dir_all(&pages).unwrap();
        let jpg = pages.join("01.jpg");
        let magick = Command::new("magick").args(["-size", "800x1200", "plasma:"]).arg(&jpg).status();
        assert!(magick.unwrap().success(), "magick is needed for this test");
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

    #[test]
    fn generates_and_caches_thumbnails() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        for ext in ["jpg", "avif"] {
            let path = comic_with_cover(dir.path(), ext);
            let key = ThumbKey { path, size: 1, modified: 1 };

            let thumb = load_or_generate(&key, &cache).unwrap();
            assert_eq!((thumb.width(), thumb.height()), (MAX_WIDTH, MAX_HEIGHT), "{ext}");
            assert!(key.cache_file(&cache).exists(), "{ext} thumbnail should be cached");
        }
    }
}
