//! Library statistics: sizes, compression savings and tagging.

use std::collections::HashMap;
use std::path::PathBuf;

use super::ComicRecord;
use super::series_check::normalize_series;

/// Comics sharing a series, publisher or format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub comics: usize,
    pub size: u64,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub comics: usize,
    pub series: usize,
    pub pages: u64,
    pub size: u64,
    /// Comics added since the "Recently Added" cutoff.
    pub recent: usize,
    /// Comics without a ComicInfo.xml.
    pub untagged: usize,
    pub untagged_size: u64,
    pub compressed: usize,
    pub uncompressed_size: u64,
    /// Comics whose size before compression was recorded, and their total
    /// size before and now.
    pub recorded: usize,
    pub recorded_before: u64,
    pub recorded_after: u64,
    /// Compressed comics with no recorded size before compression.
    pub compressed_unrecorded: usize,
    /// Largest first.
    pub formats: Vec<Group>,
    pub series_by_size: Vec<Group>,
    pub publishers: Vec<Group>,
}

impl Stats {
    pub fn saved(&self) -> u64 {
        self.recorded_before.saturating_sub(self.recorded_after)
    }
}

/// `original_sizes` are sizes before compression, by path; `recent_since`
/// is the "Recently Added" cutoff in seconds since the Unix epoch.
pub fn compute(comics: &[ComicRecord], original_sizes: &HashMap<PathBuf, u64>, recent_since: i64) -> Stats {
    let mut stats = Stats { comics: comics.len(), ..Default::default() };
    let mut formats = Grouper::default();
    let mut series = Grouper::default();
    let mut publishers = Grouper::default();

    for comic in comics {
        let m = &comic.meta;
        stats.pages += u64::from(comic.page_count);
        stats.size += comic.size;
        if comic.added >= recent_since {
            stats.recent += 1;
        }
        if !m.has_info {
            stats.untagged += 1;
            stats.untagged_size += comic.size;
        }
        match original_sizes.get(&comic.path) {
            Some(&before) => {
                stats.recorded += 1;
                stats.recorded_before += before;
                stats.recorded_after += comic.size;
            }
            None if comic.compressed => stats.compressed_unrecorded += 1,
            None => {}
        }
        if comic.compressed {
            stats.compressed += 1;
        } else {
            stats.uncompressed_size += comic.size;
        }

        formats.add(comic.format.label(), comic.format.label(), comic.size);
        if !m.series.trim().is_empty() {
            series.add(&normalize_series(&m.series), m.series.trim(), comic.size);
        }
        let publisher = m.publisher.trim();
        let publisher = if publisher.is_empty() { "Unknown" } else { publisher };
        publishers.add(&publisher.to_lowercase(), publisher, comic.size);
    }

    stats.series = series.groups.len();
    stats.formats = formats.largest_first();
    stats.series_by_size = series.largest_first();
    stats.publishers = publishers.largest_first();
    stats
}

/// Groups comics by a key, named after the first comic seen with that key.
#[derive(Default)]
struct Grouper {
    groups: HashMap<String, Group>,
}

impl Grouper {
    fn add(&mut self, key: &str, name: &str, size: u64) {
        let group =
            self.groups.entry(key.to_owned()).or_insert_with(|| Group { name: name.to_owned(), comics: 0, size: 0 });
        group.comics += 1;
        group.size += size;
    }

    fn largest_first(self) -> Vec<Group> {
        let mut groups: Vec<Group> = self.groups.into_values().collect();
        groups.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{Format, Summary};

    fn comic(path: &str, size: u64, series: &str, publisher: &str, compressed: bool) -> ComicRecord {
        let summary = Summary { format: Format::Cbz, page_count: 20, recompressible_pages: 0, comic_info: None };
        let mut comic = ComicRecord::from_summary(PathBuf::from(path), size, 1, 100, summary);
        comic.compressed = compressed;
        comic.meta.has_info = !series.is_empty();
        comic.meta.series = series.into();
        comic.meta.publisher = publisher.into();
        comic
    }

    #[test]
    fn adds_up_the_library() {
        let comics = [
            comic("/a", 60, "Saga", "Image", true),
            comic("/b", 40, "saga", "image", true),
            comic("/c", 300, "Batman", "DC Comics", false),
            comic("/d", 50, "", "", false),
        ];
        let originals = HashMap::from([(PathBuf::from("/a"), 100)]);
        let stats = compute(&comics, &originals, 50);

        assert_eq!((stats.comics, stats.series, stats.pages, stats.size, stats.recent), (4, 2, 80, 450, 4));
        assert_eq!((stats.untagged, stats.untagged_size), (1, 50));
        assert_eq!((stats.compressed, stats.uncompressed_size), (2, 350));
        assert_eq!((stats.recorded, stats.saved(), stats.compressed_unrecorded), (1, 40, 1));
        assert_eq!(stats.series_by_size[0], Group { name: "Batman".into(), comics: 1, size: 300 });
        assert_eq!(stats.series_by_size[1], Group { name: "Saga".into(), comics: 2, size: 100 });
        let publishers: Vec<&str> = stats.publishers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(publishers, ["DC Comics", "Image", "Unknown"]);
        assert_eq!(stats.formats, [Group { name: "CBZ".into(), comics: 4, size: 450 }]);
    }
}
