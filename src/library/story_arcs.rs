//! Story arcs for manga: named runs of volumes and chapters within a series.
//!
//! An arc is defined by lists of volume and chapter numbers ("3-14"), so one
//! definition fits a series whether its files are volumes, chapters or a mix
//! of both, and can be shared as a preset. Applying arcs writes `StoryArc`
//! and `StoryArcNumber` (the file's part in its arc) to each file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::ComicRecord;
use super::series_check::parse_issue_list;
use crate::anilist::MangaLookup;
use crate::archive::Format;
use crate::comicvine::normalize_name;
use crate::util::natural_cmp;

pub type Numbers = BTreeSet<u32>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Arc {
    pub name: String,
    /// The volumes it spans, e.g. "3-14". Empty for none.
    #[serde(default)]
    pub volumes: String,
    /// The chapters it spans, e.g. "17-96". Empty for none.
    #[serde(default)]
    pub chapters: String,
}

/// One series' arcs, as saved to and loaded from a preset file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub series: String,
    pub arcs: Vec<Arc>,
}

impl Preset {
    pub fn to_json(&self) -> String {
        // Serialising plain strings can't fail.
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("not a story arc preset")
    }
}

/// Reads a list like "1-6, 8". Empty text is no numbers; `None` if it isn't a list.
pub fn parse_numbers(text: &str) -> Option<Numbers> {
    if text.trim().is_empty() { Some(Numbers::new()) } else { parse_issue_list(text) }
}

/// Writes numbers compactly: "1-6, 8, 10-12".
pub fn format_numbers(numbers: &Numbers) -> String {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for &n in numbers {
        match ranges.last_mut() {
            Some((_, end)) if *end + 1 == n => *end = n,
            _ => ranges.push((n, n)),
        }
    }
    let parts: Vec<String> = ranges
        .into_iter()
        .map(|(start, end)| if start == end { start.to_string() } else { format!("{start}-{end}") })
        .collect();
    parts.join(", ")
}

/// Where a file sits in its series: a volume, a chapter, or a chapter of a
/// known volume.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
    pub volume: Option<u32>,
    pub chapter: Option<f32>,
}

impl Place {
    pub fn of(lookup: &MangaLookup) -> Self {
        Self {
            volume: lookup.volume.and_then(|v| u32::try_from(v).ok()),
            chapter: lookup.chapter.trim().parse().ok().filter(|c: &f32| c.is_finite() && *c >= 0.0),
        }
    }

    pub fn is_known(&self) -> bool {
        self.volume.is_some() || self.chapter.is_some()
    }

    /// The chapter, for matching against whole chapter numbers: 97.5 is part of 97.
    fn whole_chapter(&self) -> Option<u32> {
        self.chapter.map(|c| c.floor() as u32)
    }

    /// Volumes in order, each followed by its loose chapters, then chapters
    /// not yet collected in a volume.
    fn reading_order(&self) -> (u32, bool, f32) {
        (self.volume.unwrap_or(u32::MAX), self.chapter.is_some(), self.chapter.unwrap_or(0.0))
    }

    /// "Vol. 3", "Ch. 97", "Vol. 3, Ch. 21".
    pub fn label(&self) -> String {
        match (self.volume, self.chapter) {
            (Some(v), Some(c)) => format!("Vol. {v}, Ch. {c}"),
            (Some(v), None) => format!("Vol. {v}"),
            (None, Some(c)) => format!("Ch. {c}"),
            (None, None) => "?".to_owned(),
        }
    }
}

/// A file of a series, as the arcs dialog sees it.
#[derive(Clone, Debug)]
pub struct File {
    pub path: PathBuf,
    pub file_name: String,
    pub place: Place,
    pub story_arc: String,
    pub story_arc_number: String,
    /// CBRs can't be written.
    pub writable: bool,
}

/// One series' files, in reading order.
#[derive(Clone, Debug)]
pub struct Series {
    pub name: String,
    pub files: Vec<File>,
}

/// The series among `comics`, by name, each with its files in reading order.
pub fn series_in(comics: &[ComicRecord]) -> Vec<Series> {
    let mut by_key: BTreeMap<String, Series> = BTreeMap::new();
    for comic in comics {
        let lookup = MangaLookup::for_comic(comic);
        let file = File {
            path: comic.path.clone(),
            file_name: comic.file_name(),
            place: Place::of(&lookup),
            story_arc: comic.meta.story_arc.clone(),
            story_arc_number: comic.meta.story_arc_number.clone(),
            writable: comic.format != Format::Cbr,
        };
        let name = if lookup.series.is_empty() { "Unknown series".to_owned() } else { lookup.series };
        by_key
            .entry(normalize_name(&name))
            .or_insert_with(|| Series { name, files: Vec::new() })
            .files
            .push(file);
    }

    let mut series: Vec<Series> = by_key.into_values().collect();
    for s in &mut series {
        s.files.sort_by(|a, b| {
            a.place
                .reading_order()
                .partial_cmp(&b.place.reading_order())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| natural_cmp(&a.file_name, &b.file_name))
        });
    }
    series.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    series
}

/// An arc's volumes and chapters, read from its text.
struct ParsedArc {
    volumes: Numbers,
    chapters: Numbers,
}

fn parse_arcs(arcs: &[Arc]) -> Result<Vec<ParsedArc>, String> {
    arcs.iter()
        .enumerate()
        .map(|(i, arc)| {
            let name = arc.name.trim();
            if name.is_empty() {
                return Err(format!("Arc {} needs a name", i + 1));
            }
            let volumes = parse_numbers(&arc.volumes)
                .ok_or_else(|| format!("“{name}”: “{}” isn't a list of volumes", arc.volumes.trim()))?;
            let chapters = parse_numbers(&arc.chapters)
                .ok_or_else(|| format!("“{name}”: “{}” isn't a list of chapters", arc.chapters.trim()))?;
            Ok(ParsedArc { volumes, chapters })
        })
        .collect()
}

/// The arc a file belongs to: the first whose chapters include the file's
/// chapter, or else the first whose volumes include its volume.
fn arc_of(arcs: &[ParsedArc], place: &Place) -> Option<usize> {
    let by_chapter = place.whole_chapter().and_then(|c| arcs.iter().position(|a| a.chapters.contains(&c)));
    by_chapter.or_else(|| place.volume.and_then(|v| arcs.iter().position(|a| a.volumes.contains(&v))))
}

/// For each of `files` (in reading order), its arc's index and its part in
/// that arc, counting from 1. An error says which arc is invalid.
pub fn assign(arcs: &[Arc], files: &[File]) -> Result<Vec<Option<(usize, u32)>>, String> {
    let parsed = parse_arcs(arcs)?;
    let mut parts = vec![0u32; arcs.len()];
    Ok(files
        .iter()
        .map(|file| {
            let arc = arc_of(&parsed, &file.place)?;
            parts[arc] += 1;
            Some((arc, parts[arc]))
        })
        .collect())
}

/// The volumes and chapters that `places` stand for: a chapter for files
/// with one, otherwise a volume.
pub fn numbers_of<'a>(places: impl IntoIterator<Item = &'a Place>) -> (Numbers, Numbers) {
    let (mut volumes, mut chapters) = (Numbers::new(), Numbers::new());
    for place in places {
        match (place.whole_chapter(), place.volume) {
            (Some(chapter), _) => chapters.insert(chapter),
            (None, Some(volume)) => volumes.insert(volume),
            (None, None) => false,
        };
    }
    (volumes, chapters)
}

/// The arcs the files are tagged with now, in reading order.
pub fn arcs_from_tags(files: &[File]) -> Vec<Arc> {
    let mut names: Vec<&str> = Vec::new();
    for file in files {
        let name = file.story_arc.trim();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
        .into_iter()
        .map(|name| {
            let places = files.iter().filter(|f| f.story_arc.trim() == name).map(|f| &f.place);
            let (volumes, chapters) = numbers_of(places);
            Arc { name: name.to_owned(), volumes: format_numbers(&volumes), chapters: format_numbers(&chapters) }
        })
        .collect()
}

/// Takes `volumes` and `chapters` out of every arc, leaving lists that
/// don't read as numbers alone.
pub fn remove_numbers(arcs: &mut [Arc], volumes: &Numbers, chapters: &Numbers) {
    for arc in arcs {
        for (text, remove) in [(&mut arc.volumes, volumes), (&mut arc.chapters, chapters)] {
            if let Some(numbers) = parse_numbers(text) {
                let kept: Numbers = numbers.difference(remove).copied().collect();
                *text = format_numbers(&kept);
            }
        }
    }
}

/// Orders arcs by the first volume or chapter they include. Arcs whose
/// lists can't be read keep their place relative to each other at the end.
pub fn sort_arcs(arcs: &mut [Arc]) {
    let first = |arc: &Arc| -> (u32, u32) {
        let volume = parse_numbers(&arc.volumes).and_then(|n| n.first().copied());
        let chapter = parse_numbers(&arc.chapters).and_then(|n| n.first().copied());
        // Volumes come first in reading order, then loose chapters.
        match (volume, chapter) {
            (Some(v), _) => (0, v),
            (None, Some(c)) => (1, c),
            (None, None) => (2, 0),
        }
    };
    arcs.sort_by_key(first);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arc(name: &str, volumes: &str, chapters: &str) -> Arc {
        Arc { name: name.into(), volumes: volumes.into(), chapters: chapters.into() }
    }

    fn file(name: &str, volume: Option<u32>, chapter: Option<f32>, arc: &str) -> File {
        File {
            path: PathBuf::from(name),
            file_name: name.into(),
            place: Place { volume, chapter },
            story_arc: arc.into(),
            story_arc_number: String::new(),
            writable: true,
        }
    }

    #[test]
    fn reads_and_writes_number_lists() {
        assert_eq!(parse_numbers(""), Some(Numbers::new()));
        assert_eq!(parse_numbers("1-3, 5"), Some(Numbers::from([1, 2, 3, 5])));
        assert_eq!(parse_numbers("1-"), None);
        assert_eq!(format_numbers(&Numbers::from([1, 2, 3, 5, 7, 8])), "1-3, 5, 7-8");
    }

    #[test]
    fn places_volumes_and_chapters_in_arcs() {
        let arcs = [arc("Black Swordsman", "1-2", "1-16"), arc("Golden Age", "3-13", "17-96")];
        let files = [
            file("v01", Some(1), None, ""),
            file("v03", Some(3), None, ""),
            file("v04", Some(4), None, ""),
            file("c016", None, Some(16.0), ""),
            file("c096.5", None, Some(96.5), ""),
            file("c200", None, Some(200.0), ""),
            file("v02 c017", Some(2), Some(17.0), ""),
            file("extra", None, None, ""),
        ];
        let assigned = assign(&arcs, &files).unwrap();
        assert_eq!(
            assigned,
            [Some((0, 1)), Some((1, 1)), Some((1, 2)), Some((0, 2)), Some((1, 3)), None, Some((1, 4)), None],
            "a chapter's own number wins over its volume's"
        );
    }

    #[test]
    fn reports_invalid_arcs() {
        let files = [file("v01", Some(1), None, "")];
        assert_eq!(assign(&[arc("", "1", "")], &files), Err("Arc 1 needs a name".into()));
        assert!(assign(&[arc("A", "x", "")], &files).unwrap_err().contains("isn't a list of volumes"));
    }

    #[test]
    fn rebuilds_arcs_from_tags() {
        let files = [
            file("v01", Some(1), None, "Black Swordsman"),
            file("v02", Some(2), None, "Black Swordsman"),
            file("v03", Some(3), None, "Golden Age"),
            file("c097", None, Some(97.0), "Golden Age"),
            file("c098", None, Some(98.0), ""),
        ];
        assert_eq!(
            arcs_from_tags(&files),
            [arc("Black Swordsman", "1-2", ""), arc("Golden Age", "3", "97")]
        );
    }

    #[test]
    fn removing_numbers_keeps_the_rest() {
        let mut arcs = [arc("A", "1-5", "1-40"), arc("B", "nonsense", "")];
        remove_numbers(&mut arcs, &Numbers::from([3]), &Numbers::from([20, 21]));
        assert_eq!(arcs, [arc("A", "1-2, 4-5", "1-19, 22-40"), arc("B", "nonsense", "")]);
    }

    #[test]
    fn sorts_arcs_by_where_they_start() {
        let mut arcs = [arc("Late", "", "200-210"), arc("Middle", "5-8", ""), arc("Early", "1-4", "1-30")];
        sort_arcs(&mut arcs);
        let names: Vec<&str> = arcs.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Early", "Middle", "Late"]);
    }

    #[test]
    fn presets_round_trip() {
        let preset = Preset { series: "Berserk".into(), arcs: vec![arc("Golden Age", "3-14", "")] };
        assert_eq!(Preset::from_json(&preset.to_json()).unwrap(), preset);
        let minimal = Preset::from_json(r#"{"series": "X", "arcs": [{"name": "A", "chapters": "1-9"}]}"#).unwrap();
        assert_eq!(minimal.arcs, [arc("A", "", "1-9")]);
    }
}
