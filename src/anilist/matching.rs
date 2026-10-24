//! Working out what to search AniList for, and which result is the right one.

use super::api::Media;
use crate::comicvine::{bracketed_year, normalize_name, normalize_number, strip_bracketed};
use crate::library::ComicRecord;

/// What's known about a manga file for looking it up. Files may hold a
/// volume, a chapter, or a chapter of a known volume.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MangaLookup {
    pub series: String,
    pub volume: Option<i32>,
    /// Empty for a whole volume.
    pub chapter: String,
    pub year: Option<i32>,
}

impl MangaLookup {
    /// Uses the comic's metadata where present, otherwise its file name.
    pub fn for_comic(comic: &ComicRecord) -> Self {
        let stem = comic.path.file_stem().unwrap_or_default().to_string_lossy();
        let parsed = parse_filename(&stem);
        let m = &comic.meta;
        // ComicVine puts the series' start year in Volume, which isn't a volume number.
        let volume = m.volume.filter(|&v| v < 1000);
        Self {
            series: if m.series.is_empty() { parsed.series } else { m.series.clone() },
            volume: volume.or(parsed.volume),
            chapter: if m.number.is_empty() { parsed.chapter } else { m.number.clone() },
            year: m.year.or(parsed.year),
        }
    }
}

/// Parses names like `Berserk v01 (2003) (Digital)`, `Chainsaw Man - Chapter 97`,
/// `One Piece Vol. 3 Ch. 21` or `Berserk 001`. A number without a volume or
/// chapter marker is taken to be a chapter.
pub fn parse_filename(stem: &str) -> MangaLookup {
    let year = bracketed_year(stem);
    let cleaned = strip_bracketed(stem).replace('_', " ");
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();

    let mut volume = None;
    let mut chapter = None;
    // Where the series name ends: at the first volume or chapter marker.
    let mut series_end = tokens.len();
    let mut i = 0;
    while i < tokens.len() {
        let (marker, attached) = split_marker(tokens[i]);
        // The number is either attached (`v01`, `Ch.97`) or the next token (`Vol. 1`).
        let (number, used) = match (marker, attached) {
            (Some(_), Some(number)) => (Some(number), 1),
            (Some(_), None) => match tokens.get(i + 1) {
                Some(next) if is_number(next.trim_start_matches('#')) => (Some(next.trim_start_matches('#')), 2),
                _ => (None, 1),
            },
            (None, _) => (None, 1),
        };
        if let (Some(marker), Some(number)) = (marker, number) {
            series_end = series_end.min(i);
            match marker {
                Marker::Volume => volume = volume.or(normalize_number(number).parse().ok()),
                Marker::Chapter => chapter = chapter.or(Some(normalize_number(number))),
            }
        }
        i += used;
    }

    // Without markers, the last number is the chapter, as in `Berserk 001`.
    if volume.is_none()
        && chapter.is_none()
        && let Some(last) = tokens.iter().rposition(|t| t.starts_with('#') || is_number(t))
        && last > 0
    {
        chapter = Some(normalize_number(tokens[last]));
        series_end = last;
    }

    let series = tokens[..series_end]
        .join(" ")
        .trim_end_matches(|c: char| c == '-' || c == ',' || c == ':' || c.is_whitespace())
        .to_owned();
    MangaLookup { series, volume, chapter: chapter.unwrap_or_default(), year }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    Volume,
    Chapter,
}

/// Splits `Vol.01` into (volume marker, "01") and `Chapter` into (chapter
/// marker, none). Tokens that aren't markers give `(None, None)`.
fn split_marker(token: &str) -> (Option<Marker>, Option<&str>) {
    let lower = token.to_ascii_lowercase();
    // Longest prefixes first, so `vol.` isn't read as `v` followed by `ol.`.
    const PREFIXES: [(&str, Marker); 9] = [
        ("volume", Marker::Volume),
        ("vol.", Marker::Volume),
        ("vol", Marker::Volume),
        ("v", Marker::Volume),
        ("chapter", Marker::Chapter),
        ("chap", Marker::Chapter),
        ("ch.", Marker::Chapter),
        ("ch", Marker::Chapter),
        ("c", Marker::Chapter),
    ];
    for (prefix, marker) in PREFIXES {
        let Some(rest) = lower.strip_prefix(prefix) else { continue };
        let rest = &token[token.len() - rest.len()..];
        if rest.is_empty() {
            // A bare `c` or `v` is too likely to be a word; they need a number attached.
            return if prefix.len() > 1 { (Some(marker), None) } else { (None, None) };
        }
        if is_number(rest) {
            return (Some(marker), Some(rest));
        }
    }
    (None, None)
}

/// `12`, `012` or `97.5`.
fn is_number(token: &str) -> bool {
    let (whole, fraction) = token.split_once('.').unwrap_or((token, "0"));
    !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && !fraction.is_empty()
        && fraction.bytes().all(|b| b.is_ascii_digit())
}

/// The result that's plausibly this file's series, if there is one.
pub fn best_match<'a>(results: &'a [Media], lookup: &MangaLookup) -> Option<&'a Media> {
    let wanted = normalize_name(&lookup.series);
    if wanted.is_empty() {
        return None;
    }
    let mut scored: Vec<(i32, &Media)> = results
        .iter()
        .filter_map(|media| {
            let names: Vec<String> = media.titles().map(normalize_name).collect();
            let mut score = if names.contains(&wanted) {
                100
            } else if names.iter().any(|n| !n.is_empty() && (n.contains(&wanted) || wanted.contains(n.as_str()))) {
                50
            } else {
                return None;
            };
            // Light novels share their manga adaptation's title.
            if media.format == "NOVEL" {
                score -= 30;
            }
            if let (Some(year), Some(start)) = (lookup.year, media.start_date.year)
                && start > year
            {
                score -= 40;
            }
            Some((score, media))
        })
        .collect();
    // Stable sort: equal scores keep AniList's relevance order.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.first().filter(|(score, _)| *score >= 50).map(|(_, media)| *media)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anilist::api::{FuzzyDate, Title};

    fn lookup(series: &str, volume: Option<i32>, chapter: &str) -> MangaLookup {
        MangaLookup { series: series.into(), volume, chapter: chapter.into(), year: None }
    }

    #[test]
    fn parses_volume_and_chapter_filenames() {
        assert_eq!(parse_filename("Berserk v01 (Digital)"), lookup("Berserk", Some(1), ""));
        assert_eq!(parse_filename("Berserk - Vol. 03"), lookup("Berserk", Some(3), ""));
        assert_eq!(parse_filename("Berserk Volume 12 - The Black Swordsman"), lookup("Berserk", Some(12), ""));
        assert_eq!(parse_filename("Chainsaw Man c097"), lookup("Chainsaw Man", None, "97"));
        assert_eq!(parse_filename("Chainsaw_Man_-_Chapter_097.5"), lookup("Chainsaw Man", None, "97.5"));
        assert_eq!(parse_filename("One Piece Vol.3 Ch.21"), lookup("One Piece", Some(3), "21"));
        assert_eq!(parse_filename("One Piece v03 ch 021"), lookup("One Piece", Some(3), "21"));
        assert_eq!(parse_filename("Berserk 001"), lookup("Berserk", None, "1"));
        assert_eq!(parse_filename("86 Eighty-Six v02"), lookup("86 Eighty-Six", Some(2), ""));
        assert_eq!(parse_filename("Vinland Saga"), lookup("Vinland Saga", None, ""));
        let dated = parse_filename("Dandadan v01 (2022)");
        assert_eq!(dated.year, Some(2022));
    }

    #[test]
    fn words_starting_like_markers_stay_in_the_title() {
        assert_eq!(parse_filename("Vagabond v05").series, "Vagabond");
        assert_eq!(parse_filename("Claymore c12").series, "Claymore");
        assert_eq!(parse_filename("Chainsaw Man c3").series, "Chainsaw Man");
    }

    #[test]
    fn prefers_a_title_match_over_novels_and_later_series() {
        let media = |id, english: &str, format: &str, year| Media {
            id,
            title: Title { romaji: None, english: Some(english.into()), native: None },
            format: format.into(),
            start_date: FuzzyDate { year: Some(year), month: None },
            ..Default::default()
        };
        let results = [
            media(1, "Berserk of Gluttony", "MANGA", 2017),
            media(2, "Spice and Wolf", "NOVEL", 2006),
            media(3, "Spice and Wolf", "MANGA", 2007),
            media(4, "Unrelated", "MANGA", 2007),
        ];
        let best = |series: &str| best_match(&results, &lookup(series, None, "")).map(|m| m.id);
        assert_eq!(best("Spice & Wolf"), Some(3));
        assert_eq!(best("Berserk"), Some(1), "a partial match is better than none");
        assert_eq!(best("Nothing Like It"), None);
    }
}
