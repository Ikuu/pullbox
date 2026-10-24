//! Finding missing issues, duplicates and overlaps in each series.
//!
//! A single issue counts for its own number. A collected edition (a trade
//! paperback, "Vol. 1" and the like) counts for the issues it collects:
//! those the user entered for it, or else those its summary lists
//! ("Collects Absolute Batman #1-6"). While a trade's contents are unknown,
//! gaps in its series can only be "possibly missing".

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, Read};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use super::ComicRecord;
use crate::util::natural_cmp;

pub type Issues = BTreeSet<u32>;

/// Comics without an issue number and at least this long are taken to be
/// collected editions.
const TRADE_MIN_PAGES: u32 = 60;
/// Longer ranges are taken to be typos rather than real ones.
const MAX_RANGE: u32 = 2000;

/// What a comic is, as far as the series check is concerned.
#[derive(Debug, PartialEq, Eq)]
pub enum Kind {
    Single(u32),
    Trade,
    /// No usable issue number, e.g. "1.MU" or an untitled one-shot.
    Other,
}

/// Classifies a comic. Comics the user entered contents for are always trades.
pub fn classify(comic: &ComicRecord, has_manual_contents: bool) -> Kind {
    let m = &comic.meta;
    let unnumbered_and_long = m.number.trim().is_empty() && comic.page_count >= TRADE_MIN_PAGES;
    if has_manual_contents || looks_like_volume_title(&m.title) || unnumbered_and_long {
        return Kind::Trade;
    }
    match issue_number(&m.number) {
        Some(n) => Kind::Single(n),
        None => Kind::Other,
    }
}

/// "Vol. 1: The Zoo", "Volume 2", "vol 3".
fn looks_like_volume_title(title: &str) -> bool {
    let title = title.trim_start().to_lowercase();
    ["volume", "vol.", "vol "].iter().any(|prefix| {
        title.strip_prefix(prefix).is_some_and(|rest| rest.trim_start().starts_with(|c: char| c.is_ascii_digit()))
    })
}

/// A whole issue number: "7" or "007". Not "1.MU", "½" or "7a".
pub fn issue_number(number: &str) -> Option<u32> {
    let number = number.trim();
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// Reads a list like "1-6, 8, 10–12". `None` if it isn't one.
pub fn parse_issue_list(text: &str) -> Option<Issues> {
    let mut issues = Issues::new();
    for part in text.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()) {
        let (range, rest) = leading_range(part.trim_start_matches('#'))?;
        if !rest.trim().is_empty() {
            return None;
        }
        issues.extend(range);
    }
    (!issues.is_empty()).then_some(issues)
}

/// Writes issues compactly: "#1–6, #8, #10–12".
pub fn format_issues(issues: &Issues) -> String {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for &n in issues {
        match ranges.last_mut() {
            Some((_, end)) if *end + 1 == n => *end = n,
            _ => ranges.push((n, n)),
        }
    }
    let parts: Vec<String> = ranges
        .into_iter()
        .map(|(start, end)| if start == end { format!("#{start}") } else { format!("#{start}–{end}") })
        .collect();
    parts.join(", ")
}

/// A number or range at the start of `text` ("1-6", "7 through 12"), and
/// what follows it.
fn leading_range(text: &str) -> Option<(RangeInclusive<u32>, &str)> {
    let (start, rest) = leading_number(text.trim_start())?;
    let after = rest.trim_start();
    for separator in ["-", "–", "—", "to ", "through ", "thru "] {
        if let Some(end_text) = after.strip_prefix(separator)
            && let Some((end, rest)) = leading_number(end_text.trim_start().trim_start_matches('#'))
        {
            let valid = start <= end && end - start <= MAX_RANGE;
            return valid.then_some((start..=end, rest));
        }
    }
    Some((start..=start, rest))
}

fn leading_number(text: &str) -> Option<(u32, &str)> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let number = text[..digits].parse().ok()?;
    Some((number, &text[digits..]))
}

/// The issues of `series` that a summary says the comic collects, as in
/// "Collects Absolute Batman #1-6 and Annual #1" or "collecting issues 7
/// through 12". Issues of other series it names are left out. `None` if the
/// summary doesn't say.
pub fn issues_from_summary(summary: &str, series: &str) -> Option<Issues> {
    let text = summary.to_lowercase();
    let own = normalize_series(series);
    let mut issues = Issues::new();
    let mut search = 0;
    while let Some(found) = text[search..].find("collect") {
        let start = search + found;
        read_collects_clause(clause_at(&text[start..]), &own, &mut issues);
        search = start + "collect".len();
    }
    (!issues.is_empty()).then_some(issues)
}

/// The rest of the sentence or bracket starting at `text`. Brackets opened
/// within it, as in "(2024)", don't end it.
fn clause_at(text: &str) -> &str {
    let mut depth = 0u32;
    for (i, c) in text.char_indices() {
        let next = text[i + c.len_utf8()..].chars().next();
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth == 0 => return &text[..i],
            ')' | ']' => depth -= 1,
            '\n' => return &text[..i],
            '.' if next.is_none_or(char::is_whitespace) => return &text[..i],
            _ => {}
        }
    }
    text
}

/// Adds the issues of the series named `own` (normalised) listed in a
/// clause such as "collects absolute batman #1-6, 8 and annual #1".
fn read_collects_clause(clause: &str, own: &str, issues: &mut Issues) {
    // Which series the numbers belong to; none named yet means the comic's own.
    let mut current_is_own = true;
    for segment in split_list(clause) {
        let (name, numbers) = match segment.find('#') {
            Some(hash) => (&segment[..hash], &segment[hash + 1..]),
            None => {
                // "issues 7 through 12", or a bare "8" continuing a list.
                let text = strip_lead_words(segment);
                if text.starts_with(|c: char| c.is_ascii_digit()) { ("", text) } else { continue }
            }
        };
        let name = normalize_series(strip_lead_words(name));
        if !name.is_empty() {
            current_is_own = name == own;
        }
        if current_is_own && let Some((range, _)) = leading_range(numbers) {
            issues.extend(range);
        }
    }
}

/// Splits on commas, semicolons, "&" and " and ".
fn split_list(clause: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < clause.len() {
        let rest = &clause[i..];
        let separator = [",", ";", "&", " and "].into_iter().find(|s| rest.starts_with(s));
        match separator {
            Some(s) => {
                parts.push(&clause[start..i]);
                i += s.len();
                start = i;
            }
            None => i += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    parts.push(&clause[start..]);
    parts
}

/// Drops words that introduce a list rather than name a series.
fn strip_lead_words(text: &str) -> &str {
    const LEAD_WORDS: [&str; 10] =
        ["collects", "collecting", "collected", "collect", "from", "in", "issues", "issue", "the", ":"];
    let mut text = text.trim();
    while let Some(rest) = LEAD_WORDS.iter().find_map(|w| {
        text.strip_prefix(w).filter(|r| r.is_empty() || r.starts_with(|c: char| !c.is_alphanumeric()))
    }) {
        text = rest.trim_start_matches(|c: char| c == ':' || c.is_whitespace());
    }
    text
}

/// Lower case, without a leading "the", bracketed parts like "(2024)" or
/// punctuation, so names written slightly differently still match.
pub fn normalize_series(name: &str) -> String {
    let mut plain = String::new();
    let mut depth = 0u32;
    for c in name.to_lowercase().chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            c if depth > 0 || !(c.is_alphanumeric() || c.is_whitespace()) => plain.push(' '),
            c => plain.push(c),
        }
    }
    let words: Vec<&str> = plain.split_whitespace().collect();
    let words = if words.first() == Some(&"the") { &words[1..] } else { &words[..] };
    words.join(" ")
}

// ----- the report -----

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Contents {
    /// Entered by the user.
    Entered(Issues),
    /// Read from the comic's summary.
    FromSummary(Issues),
    Unknown,
}

impl Contents {
    pub fn issues(&self) -> Option<&Issues> {
        match self {
            Contents::Entered(issues) | Contents::FromSummary(issues) => Some(issues),
            Contents::Unknown => None,
        }
    }
}

#[derive(Debug)]
pub struct Trade {
    pub path: PathBuf,
    pub contents: Contents,
}

#[derive(Debug)]
pub struct SeriesReport {
    pub series: String,
    pub comic_count: usize,
    /// Issues from #1 to the highest one owned that aren't in the library.
    pub missing: Issues,
    /// Some trade's contents are unknown, so it may hold the missing issues.
    pub uncertain: bool,
    pub trades: Vec<Trade>,
    /// Issue numbers held as more than one single issue.
    pub duplicates: Vec<(u32, Vec<PathBuf>)>,
    /// Single issues that a trade also collects: (issue, single, trade).
    pub overlaps: Vec<(u32, PathBuf, PathBuf)>,
    /// The highest issue number owned, if any.
    pub highest: Option<u32>,
}

impl SeriesReport {
    pub fn has_findings(&self) -> bool {
        !self.missing.is_empty() || self.uncertain || !self.duplicates.is_empty() || !self.overlaps.is_empty()
    }
}

/// What the check covered.
#[derive(Debug, Default)]
pub struct Report {
    pub series: Vec<SeriesReport>,
    /// Comics without a series name, which can't be checked.
    pub untagged: usize,
    /// Comics in a series but with no usable issue number.
    pub unnumbered: usize,
}

/// Trades whose contents have to come from their summary: those the user
/// hasn't entered contents for.
pub fn trades_needing_summaries(comics: &[ComicRecord], entered: &HashMap<PathBuf, String>) -> Vec<PathBuf> {
    comics
        .iter()
        .filter(|c| !c.meta.series.is_empty() && c.meta.has_info && !entered.contains_key(&c.path))
        .filter(|c| classify(c, false) == Kind::Trade)
        .map(|c| c.path.clone())
        .collect()
}

/// Checks every series among `comics`. `entered` holds the contents the
/// user gave for trades; `summaries` the summaries of the others.
pub fn check(comics: &[ComicRecord], entered: &HashMap<PathBuf, String>, summaries: &HashMap<PathBuf, String>) -> Report {
    let mut report = Report::default();
    let mut groups: BTreeMap<String, Vec<&ComicRecord>> = BTreeMap::new();
    for comic in comics {
        if comic.meta.series.trim().is_empty() {
            report.untagged += 1;
        } else {
            groups.entry(normalize_series(&comic.meta.series)).or_default().push(comic);
        }
    }

    for comics in groups.into_values() {
        let mut singles: BTreeMap<u32, Vec<PathBuf>> = BTreeMap::new();
        let mut trades = Vec::new();
        for comic in &comics {
            let manual = entered.get(&comic.path);
            match classify(comic, manual.is_some()) {
                Kind::Single(n) => singles.entry(n).or_default().push(comic.path.clone()),
                Kind::Other => report.unnumbered += 1,
                Kind::Trade => {
                    let contents = match (manual.and_then(|m| parse_issue_list(m)), summaries.get(&comic.path)) {
                        (Some(issues), _) => Contents::Entered(issues),
                        (None, Some(summary)) => issues_from_summary(summary, &comic.meta.series)
                            .map_or(Contents::Unknown, Contents::FromSummary),
                        (None, None) => Contents::Unknown,
                    };
                    trades.push(Trade { path: comic.path.clone(), contents });
                }
            }
        }
        if singles.is_empty() && trades.is_empty() {
            continue;
        }

        let mut owned: Issues = singles.keys().copied().collect();
        for trade in &trades {
            owned.extend(trade.contents.issues().into_iter().flatten());
        }
        let highest = owned.last().copied();
        let missing = (1..=highest.unwrap_or(0)).filter(|n| !owned.contains(n)).collect();

        let mut overlaps = Vec::new();
        for (&n, paths) in &singles {
            for trade in trades.iter().filter(|t| t.contents.issues().is_some_and(|i| i.contains(&n))) {
                overlaps.push((n, paths[0].clone(), trade.path.clone()));
            }
        }
        trades.sort_by(|a, b| natural_cmp(&file_name(&a.path), &file_name(&b.path)));

        report.series.push(SeriesReport {
            series: comics[0].meta.series.clone(),
            comic_count: comics.len(),
            missing,
            uncertain: trades.iter().any(|t| t.contents == Contents::Unknown),
            duplicates: singles.into_iter().filter(|(_, paths)| paths.len() > 1).collect(),
            trades,
            overlaps,
            highest,
        });
    }
    report.series.sort_by(|a, b| natural_cmp(&a.series, &b.series));
    report
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

// ----- identical files -----

/// Groups of files with exactly the same contents. Only files of the same
/// size are compared, so most files are never read.
pub fn identical_files(files: &[(PathBuf, u64)], progress: impl Fn(usize, usize)) -> Vec<Vec<PathBuf>> {
    let mut by_size: HashMap<u64, Vec<&PathBuf>> = HashMap::new();
    for (path, size) in files.iter().filter(|(_, size)| *size > 0) {
        by_size.entry(*size).or_default().push(path);
    }
    let candidates: Vec<Vec<&PathBuf>> = by_size.into_values().filter(|paths| paths.len() > 1).collect();

    let mut groups = Vec::new();
    for (i, mut paths) in candidates.into_iter().enumerate() {
        progress(i, 0);
        // Split the same-size files into groups of identical ones.
        while let Some(first) = paths.pop() {
            let (same, different): (Vec<&PathBuf>, Vec<&PathBuf>) =
                paths.into_iter().partition(|other| same_contents(first, other).unwrap_or(false));
            if !same.is_empty() {
                let mut group: Vec<PathBuf> = same.into_iter().cloned().collect();
                group.push(first.clone());
                group.sort();
                groups.push(group);
            }
            paths = different;
        }
    }
    groups.sort();
    groups
}

fn same_contents(a: &Path, b: &Path) -> io::Result<bool> {
    let (mut a, mut b) = (File::open(a)?, File::open(b)?);
    let (mut buf_a, mut buf_b) = (vec![0; 1 << 16], vec![0; 1 << 16]);
    loop {
        let read = read_full(&mut a, &mut buf_a)?;
        if read != read_full(&mut b, &mut buf_b)? || buf_a[..read] != buf_b[..read] {
            return Ok(false);
        }
        if read == 0 {
            return Ok(true);
        }
    }
}

/// Fills `buf` unless the file ends first; returns how much was read.
fn read_full(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..])? {
            0 => break,
            n => total += n,
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{Format, Summary};

    fn comic(path: &str, series: &str, number: &str, title: &str, pages: u32) -> ComicRecord {
        let summary = Summary { format: Format::Cbz, page_count: pages, recompressible_pages: 0, comic_info: None };
        let mut comic = ComicRecord::from_summary(PathBuf::from(path), 1, 1, 1, summary);
        comic.meta.has_info = true;
        comic.meta.series = series.into();
        comic.meta.number = number.into();
        comic.meta.title = title.into();
        comic
    }

    fn issues(list: &str) -> Issues {
        parse_issue_list(list).unwrap()
    }

    #[test]
    fn tells_trades_from_single_issues() {
        assert_eq!(classify(&comic("/a", "S", "019", "", 28), false), Kind::Single(19));
        assert_eq!(classify(&comic("/a", "S", "", "Vol. 1: The Zoo", 180), false), Kind::Trade);
        assert_eq!(classify(&comic("/a", "S", "1", "Volume 2", 150), false), Kind::Trade);
        assert_eq!(classify(&comic("/a", "S", "", "", 200), false), Kind::Trade);
        assert_eq!(classify(&comic("/a", "S", "", "Volcano Special", 30), false), Kind::Other);
        assert_eq!(classify(&comic("/a", "S", "1.MU", "", 30), false), Kind::Other);
        assert_eq!(classify(&comic("/a", "S", "3", "", 30), true), Kind::Trade);
    }

    #[test]
    fn reads_issue_lists() {
        assert_eq!(issues("1-6, 8, #10–12"), [1, 2, 3, 4, 5, 6, 8, 10, 11, 12].into());
        assert_eq!(issues("7 to 9"), [7, 8, 9].into());
        assert_eq!(parse_issue_list("1-6 and more"), None);
        assert_eq!(parse_issue_list("6-1"), None);
        assert_eq!(parse_issue_list(""), None);
        assert_eq!(format_issues(&issues("1-6, 8, 10-11")), "#1–6, #8, #10–11");
    }

    #[test]
    fn reads_collected_issues_from_summaries() {
        let read = |summary| issues_from_summary(summary, "Absolute Batman").map(|i| format_issues(&i));
        assert_eq!(read("The hit series! Collects Absolute Batman #1-6."), Some("#1–6".into()));
        assert_eq!(read("Collecting ABSOLUTE BATMAN (2024) #7–12, 14 and the Annual #1"), Some("#7–12, #14".into()));
        assert_eq!(read("Collects Absolute Batman #1-6 and Batman #10."), Some("#1–6".into()));
        assert_eq!(read("Collects Batman #10 and Absolute Batman #1-3"), Some("#1–3".into()));
        assert_eq!(read("Bruce returns. (Collects issues 13 through 18)"), Some("#13–18".into()));
        assert_eq!(read("A collection of stories, vol. 2."), None);
        assert_eq!(read("No list here."), None);
    }

    fn check_one(comics: &[ComicRecord], entered: &[(&str, &str)], summaries: &[(&str, &str)]) -> SeriesReport {
        let entered = entered.iter().map(|(p, i)| (PathBuf::from(p), i.to_string())).collect();
        let summaries = summaries.iter().map(|(p, s)| (PathBuf::from(p), s.to_string())).collect();
        let mut report = check(comics, &entered, &summaries);
        assert_eq!(report.series.len(), 1);
        report.series.remove(0)
    }

    #[test]
    fn a_trade_and_the_singles_after_it_are_complete() {
        let mut comics = vec![comic("/v1", "Saga", "", "Vol. 1", 160)];
        comics.extend((7..=12).map(|n| comic(&format!("/{n}"), "Saga", &n.to_string(), "", 28)));
        let report = check_one(&comics, &[("/v1", "1-6")], &[]);
        assert!(report.missing.is_empty() && !report.uncertain && !report.has_findings());
        assert_eq!(report.highest, Some(12));
    }

    #[test]
    fn finds_an_issue_between_a_trade_and_singles() {
        let mut comics = vec![comic("/v1", "Saga", "", "Vol. 1", 160)];
        comics.extend((8..=12).map(|n| comic(&format!("/{n}"), "Saga", &n.to_string(), "", 28)));
        let report = check_one(&comics, &[], &[("/v1", "Collects Saga #1-6.")]);
        assert_eq!(format_issues(&report.missing), "#7");
        assert!(!report.uncertain);
        assert_eq!(report.trades[0].contents, Contents::FromSummary(issues("1-6")));
    }

    #[test]
    fn gaps_are_uncertain_while_a_trade_is_unknown() {
        let mut comics = vec![comic("/v1", "Saga", "", "Vol. 1", 160)];
        comics.extend((8..=9).map(|n| comic(&format!("/{n}"), "Saga", &n.to_string(), "", 28)));
        let report = check_one(&comics, &[], &[]);
        assert_eq!(format_issues(&report.missing), "#1–7");
        assert!(report.uncertain);
        assert_eq!(report.trades[0].contents, Contents::Unknown);
    }

    #[test]
    fn finds_duplicates_and_overlaps() {
        let comics = vec![
            comic("/v1", "The Saga", "", "Vol. 1", 160),
            comic("/a/3", "Saga", "3", "", 28),
            comic("/b/3", "saga", "003", "", 28),
            comic("/7", "Saga (2012)", "7", "", 28),
        ];
        let report = check_one(&comics, &[("/v1", "1-6")], &[]);
        assert_eq!(report.duplicates, vec![(3, vec![PathBuf::from("/a/3"), PathBuf::from("/b/3")])]);
        assert_eq!(report.overlaps, vec![(3, PathBuf::from("/a/3"), PathBuf::from("/v1"))]);
        assert!(report.missing.is_empty());
    }

    #[test]
    fn entered_contents_win_over_the_summary() {
        let comics = vec![comic("/v1", "Saga", "", "Vol. 1", 160), comic("/9", "Saga", "9", "", 28)];
        let report = check_one(&comics, &[("/v1", "1-8")], &[("/v1", "Collects Saga #1-6")]);
        assert!(report.missing.is_empty());
        assert_eq!(report.trades[0].contents, Contents::Entered(issues("1-8")));
    }

    #[test]
    fn finds_identical_files() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, contents: &[u8]| {
            let path = dir.path().join(name);
            std::fs::write(&path, contents).unwrap();
            (path, contents.len() as u64)
        };
        let files = [write("a", b"same"), write("b", b"same"), write("c", b"diff"), write("d", b"other")];
        let groups = identical_files(&files, |_, _| {});
        assert_eq!(groups, vec![vec![files[0].0.clone(), files[1].0.clone()]]);
    }
}
