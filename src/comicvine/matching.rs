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

    // Volume markers like `v2` / `Vol. 2` aren't part of the series name.
    let series_tokens: Vec<&str> = tokens
        .into_iter()
        .filter(|t| !is_volume_marker(t))
        .collect();
    let series = series_tokens
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

fn bracketed_year(text: &str) -> Option<i32> {
    text.split(['(', '['])
        .skip(1)
        .filter_map(|part| part.split([')', ']']).next())
        .filter_map(|inner| inner.trim().parse::<i32>().ok())
        .find(|year| (1900..=2100).contains(year))
}

fn strip_bracketed(text: &str) -> String {
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

fn is_volume_marker(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("vol.")
        .or_else(|| lower.strip_prefix("vol"))
        .or_else(|| lower.strip_prefix('v'));
    matches!(rest, Some(r) if r.is_empty() || r.chars().all(|c| c.is_ascii_digit()))
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
}
