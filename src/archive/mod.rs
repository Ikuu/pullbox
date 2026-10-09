//! Read access to comic archives (CBZ and CBR).

mod cbr;
mod cbz;

use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Result, bail};

use crate::comicinfo::{self, ComicInfo};
use crate::util::{lower_ext, natural_cmp};

/// Archive container format, detected from the file's magic bytes so that
/// mislabelled files (a `.cbr` that is really a zip) are handled correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Cbz,
    Cbr,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Cbz => "CBZ",
            Format::Cbr => "CBR",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "CBZ" => Some(Format::Cbz),
            "CBR" => Some(Format::Cbr),
            _ => None,
        }
    }

    pub fn detect(path: &Path) -> Result<Self> {
        let mut magic = [0u8; 4];
        let read = File::open(path)?.read(&mut magic)?;
        Ok(match &magic[..read] {
            b"PK\x03\x04" => Format::Cbz,
            b"Rar!" => Format::Cbr,
            _ if lower_ext(path) == "cbr" => Format::Cbr,
            _ => Format::Cbz,
        })
    }
}

/// File extensions treated as comic archives when scanning.
pub const COMIC_EXTENSIONS: [&str; 4] = ["cbz", "cbr", "zip", "rar"];

const IMAGE_EXTENSIONS: [&str; 9] = [
    "jpg", "jpeg", "png", "gif", "webp", "avif", "bmp", "jxl", "tif",
];

pub fn is_comic_file(path: &Path) -> bool {
    COMIC_EXTENSIONS.contains(&lower_ext(path).as_str())
}

/// Image formats AVIF compression re-encodes. GIF, WebP and AVIF are left alone.
const RECOMPRESSIBLE_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "bmp", "tiff", "tif"];

fn is_image_entry(name: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&lower_ext(Path::new(name)).as_str())
}

/// Whether AVIF compression would re-encode this image.
pub fn is_recompressible(path: impl AsRef<Path>) -> bool {
    RECOMPRESSIBLE_EXTENSIONS.contains(&lower_ext(path.as_ref()).as_str())
}

/// Image counts for an archive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pages {
    total: u32,
    /// Pages AVIF compression would still re-encode.
    recompressible: u32,
}

impl Pages {
    fn count<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut pages = Self::default();
        for name in names.into_iter().filter(|n| is_image_entry(n)) {
            pages.total += 1;
            pages.recompressible += u32::from(is_recompressible(name));
        }
        pages
    }
}

/// The cover page: the first image in natural order (`2.jpg` before `10.jpg`),
/// ignoring macOS resource-fork junk.
fn cover_entry<'a>(names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    names
        .filter(|name| is_image_entry(name) && !name.starts_with("__MACOSX"))
        .min_by(|a, b| natural_cmp(a, b))
}

/// True when an archive entry is the ComicInfo.xml metadata file.
fn is_comicinfo_entry(name: &str) -> bool {
    name.eq_ignore_ascii_case(comicinfo::FILE_NAME)
}

/// What the library needs to know about an archive.
#[derive(Debug)]
pub struct Summary {
    pub format: Format,
    pub page_count: u32,
    /// Pages AVIF compression would still re-encode (JPEG, PNG, …).
    pub recompressible_pages: u32,
    pub comic_info: Option<ComicInfo>,
}

pub fn read_summary(path: &Path) -> Result<Summary> {
    let (format, pages, xml) = read_raw(path)?;
    // A malformed ComicInfo.xml shouldn't stop the comic from being listed.
    let comic_info = xml.and_then(|xml| ComicInfo::parse(&xml).ok());
    Ok(Summary {
        format,
        page_count: pages.total,
        recompressible_pages: pages.recompressible,
        comic_info,
    })
}

/// Reads the full metadata for editing. A comic without a ComicInfo.xml yields
/// an empty `ComicInfo`; a malformed one is an error, so it isn't overwritten.
pub fn read_comic_info(path: &Path) -> Result<ComicInfo> {
    let (_, _, xml) = read_raw(path)?;
    Ok(match xml {
        Some(xml) => ComicInfo::parse(&xml)?,
        None => ComicInfo::default(),
    })
}

/// Replaces the archive's ComicInfo.xml. Only CBZ archives can be written.
pub fn write_comic_info(path: &Path, info: &ComicInfo) -> Result<()> {
    match Format::detect(path)? {
        Format::Cbz => cbz::write_comic_info(path, &info.to_xml()),
        Format::Cbr => bail!("CBR archives are read-only; convert to CBZ first"),
    }
}

/// Raw bytes of the cover image, or `None` if the archive has no images.
pub fn read_cover(path: &Path) -> Result<Option<Vec<u8>>> {
    match Format::detect(path)? {
        Format::Cbz => cbz::read_cover(path),
        Format::Cbr => cbr::read_cover(path),
    }
}

/// Extracts every entry of the archive into `dest`.
pub fn extract_all(path: &Path, dest: &Path) -> Result<()> {
    match Format::detect(path)? {
        Format::Cbz => cbz::extract_all(path, dest),
        Format::Cbr => cbr::extract_all(path, dest),
    }
}

pub use cbz::pack_directory;

/// Format, image counts and raw ComicInfo.xml.
fn read_raw(path: &Path) -> Result<(Format, Pages, Option<String>)> {
    let format = Format::detect(path)?;
    let (pages, xml) = match format {
        Format::Cbz => cbz::read_summary(path)?,
        Format::Cbr => cbr::read_summary(path)?,
    };
    Ok((format, pages, xml))
}
