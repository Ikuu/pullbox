//! Working out what to search for, and which ComicVine series is the right one.

use super::api::Volume;
use crate::library::ComicRecord;

/// What's known about a comic for looking it up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lookup {
    pub series: String,
    pub number: String,
    pub year: Option<i32>,
}

impl Lookup {
    /// Uses the comic's metadata where present, otherwise its file name.
    pub fn for_comic(comic: &ComicRecord) -> Self {
        let stem = comic.path.file_stem().unwrap_or_default().to_string_lossy();
        let parsed = parse_filename(&stem);
        let m = &comic.meta;
        Self {
            series: if m.series.is_empty() { parsed.series } else { m.series.clone() },
            number: if m.number.is_empty() { parsed.number } else { m.number.clone() },
            year: m.year.or(parsed.year),
        }
    }
}

/// Parses names like `Saga 012 (2013) (Digital) [Group]` or `Batman_-_v2_#5`.
pub fn parse_filename(stem: &str) -> Lookup {
    let year = bracketed_year(stem);
    let cleaned = strip_bracketed(stem).replace('_', " ");
    let mut tokens: Vec<&str> = cleaned.split_whitespace().collect();

    // The issue number is the last numeric token, e.g. `012`, `#5` or `1.5`.
    let number = tokens.iter().rposition(|t| is_issue_number(t)).map(|i| {
        let number = normalize_number(tokens[i]);
        tokens.truncate(i);
        number
    });

    // A word introducing the number, as in `Batman Issue 14`.
    if number.is_some() && tokens.len() > 1 && tokens.last().is_some_and(|t| t.eq_ignore_ascii_case("issue")) {
        tokens.pop();
    }

    let series = without_volume_markers(&tokens)
        .join(" ")
        .trim_end_matches(|c: char| c == '-' || c == ',' || c == ':' || c.is_whitespace())
        .to_owned();

    Lookup { series, number: number.unwrap_or_default(), year }
}

/// `012` → `12`, `#5` → `5`, `001.5` → `1.5`; other forms are left alone.
pub fn normalize_number(number: &str) -> String {
    let number = number.trim().trim_start_matches('#');
    let (whole, fraction) = number.split_once('.').map_or((number, None), |(w, f)| (w, Some(f)));
    if whole.is_empty() || !whole.chars().all(|c| c.is_ascii_digit()) {
        return number.to_owned();
    }
    let whole = match whole.trim_start_matches('0') {
        "" => "0",
        trimmed => trimmed,
    };
    match fraction {
        Some(fraction) => format!("{whole}.{fraction}"),
        None => whole.to_owned(),
    }
}

/// Lower-case, `&` → `and`, no leading "the", punctuation removed.
pub fn normalize_name(name: &str) -> String {
    let name = name.to_lowercase().replace('&', " and ");
    let words: Vec<String> = name
        .split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|w| !w.is_empty())
        .collect();
    let words = match words.first().map(String::as_str) {
        Some("the") => &words[1..],
        _ => &words[..],
    };
    words.join(" ")
}

/// Volumes that plausibly are this comic's series, best first.
pub fn rank_volumes<'a>(volumes: &'a [Volume], lookup: &Lookup) -> Vec<&'a Volume> {
    let wanted = normalize_name(&lookup.series);
    let number: Option<f32> = lookup.number.parse().ok();

    let mut scored: Vec<(i32, &Volume)> = volumes
        .iter()
        .filter_map(|volume| {
            let name = normalize_name(&volume.name);
            let mut score = if name == wanted {
                100
            } else if !wanted.is_empty() && (name.contains(&wanted) || wanted.contains(&name)) {
                50
            } else {
                return None;
            };

            if let (Some(year), Some(start)) = (lookup.year, volume.start_year()) {
                // A series can't contain an issue published before it started.
                score += if start <= year { 30 - (year - start).min(30) } else { -40 };
            }
            if let Some(number) = number
                && volume.count_of_issues > 0
            {
                score += if volume.count_of_issues as f32 >= number { 10 } else { -20 };
            }
            Some((score, volume))
        })
        .collect();

    // Stable sort: equal scores keep ComicVine's relevance order.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, volume)| volume).collect()
}

pub fn bracketed_year(text: &str) -> Option<i32> {
    text.split(['(', '['])
        .skip(1)
        .filter_map(|part| part.split([')', ']']).next())
        .filter_map(|inner| inner.trim().parse::<i32>().ok())
        .find(|year| (1900..=2100).contains(year))
}

pub fn strip_bracketed(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn is_issue_number(token: &str) -> bool {
    let token = token.trim_start_matches('#');
    let mut parts = token.splitn(2, '.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next().unwrap_or("0");
    !whole.is_empty() && whole.chars().all(|c| c.is_ascii_digit()) && fraction.chars().all(|c| c.is_ascii_digit())
}

/// Removes markers like `v2`, `Vol. 2` and `Volume 01`, which aren't part of
/// the series name. A word on its own only counts when a number follows or,
/// for `Vol` / `Volume`, when it ends the name (the number having been taken
/// as the issue number), so `V for Vendetta` and `Batman v Superman` survive.
fn without_volume_markers<'a>(tokens: &[&'a str]) -> Vec<&'a str> {
    let mut kept = Vec::with_capacity(tokens.len());
    let mut rest = tokens.iter().copied().peekable();
    while let Some(token) = rest.next() {
        let marker = volume_marker_suffix(token);
        if matches!(marker.as_deref(), Some(suffix) if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        if marker.as_deref() == Some("") && !kept.is_empty() {
            match rest.peek() {
                Some(next) if is_issue_number(next) => {
                    rest.next();
                    continue;
                }
                None => continue,
                Some(_) => {}
            }
        }
        kept.push(token);
    }
    kept
}

/// What follows a volume prefix: `Vol.2` → `2`, `Volume` → an empty string.
fn volume_marker_suffix(token: &str) -> Option<String> {
    let lower = token.to_ascii_lowercase();
    ["volume", "vol.", "vol", "v"]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(series: &str, number: &str, year: Option<i32>) -> Lookup {
        Lookup { series: series.into(), number: number.into(), year }
    }

    #[test]
    fn parses_common_filenames() {
        assert_eq!(parse_filename("Saga 012 (2013) (Digital) [Group]"), lookup("Saga", "12", Some(2013)));
        assert_eq!(parse_filename("Batman_-_v2_#005"), lookup("Batman", "5", None));
        assert_eq!(parse_filename("The Walking Dead 001.5 (of 193)"), lookup("The Walking Dead", "1.5", None));
        assert_eq!(parse_filename("Watchmen"), lookup("Watchmen", "", None));
        assert_eq!(parse_filename("2000 AD 1234 (2021)"), lookup("2000 AD", "1234", Some(2021)));
    }

    #[test]
    fn strips_volume_and_issue_words() {
        assert_eq!(parse_filename("Batman (2025) Volume 01 Issue 014"), lookup("Batman", "14", Some(2025)));
        assert_eq!(parse_filename("Batman Vol. 2 #5"), lookup("Batman", "5", None));
        assert_eq!(parse_filename("Batman Vol.2 Issue 5"), lookup("Batman", "5", None));
        assert_eq!(parse_filename("Batman Volume 2"), lookup("Batman", "2", None));
    }

    #[test]
    fn keeps_titles_that_only_look_like_markers() {
        assert_eq!(parse_filename("V for Vendetta 3"), lookup("V for Vendetta", "3", None));
        assert_eq!(parse_filename("Batman v Superman 3"), lookup("Batman v Superman", "3", None));
        assert_eq!(parse_filename("Volumes of Blood 2"), lookup("Volumes of Blood", "2", None));
        assert_eq!(parse_filename("Issue 5"), lookup("Issue", "5", None));
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize_number("007"), "7");
        assert_eq!(normalize_number("0"), "0");
        assert_eq!(normalize_number("#01.5"), "1.5");
        assert_eq!(normalize_number("1/2"), "1/2");
        assert_eq!(normalize_name("The Amazing Spider-Man"), normalize_name("amazing spiderman"));
        assert_eq!(normalize_name("Mr. & Mrs. X"), "mr and mrs x");
    }

    #[test]
    fn prefers_exact_name_and_plausible_year() {
        let volume = |id, name: &str, year: &str, count| Volume {
            id,
            name: name.into(),
            start_year: Some(year.into()),
            publisher: None,
            count_of_issues: count,
            image: None,
        };
        let volumes = [
            volume(1, "Saga of the Swamp Thing", "1982", 39),
            volume(2, "Saga", "2012", 66),
            volume(3, "Saga", "1990", 3),
            volume(4, "Unrelated", "2012", 10),
        ];
        let ranked: Vec<u64> = rank_volumes(&volumes, &lookup("Saga", "12", Some(2013))).iter().map(|v| v.id).collect();
        assert_eq!(ranked, [2, 3, 1]);
    }

    #[test]
    fn ignores_publishers_by_name() {
        let volume = |publisher: Option<&str>| Volume {
            id: 1,
            name: "Captain America".into(),
            start_year: Some("2009".into()),
            publisher: publisher.map(|name| crate::comicvine::api::Named { name: name.into() }),
            count_of_issues: 23,
            image: None,
        };
        let ignored = ["Editorial Televisa".to_owned()];
        assert!(volume(Some("editorial televisa")).publisher_ignored(&ignored));
        assert!(!volume(Some("Marvel")).publisher_ignored(&ignored));
        assert!(!volume(None).publisher_ignored(&ignored));
    }
}
