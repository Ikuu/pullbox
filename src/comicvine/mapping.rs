//! Turning a ComicVine issue into ComicInfo.xml field values.

use super::api::{Credit, Issue, Volume};

/// `(ComicInfo element, value)` for every field ComicVine has a value for.
/// Fields ComicVine knows nothing about are left out, so they aren't cleared.
pub fn comicinfo_values(volume: &Volume, issue: &Issue) -> Vec<(&'static str, String)> {
    let mut values = vec![
        ("Series", volume.name.clone()),
        ("Number", issue.issue_number.clone()),
        ("Volume", volume.start_year().map(|y| y.to_string()).unwrap_or_default()),
        ("Title", issue.name.clone().unwrap_or_default()),
        ("Publisher", volume.publisher_name().to_owned()),
        ("Summary", issue.description.as_deref().map(html_to_text).unwrap_or_default()),
        ("Web", issue.site_detail_url.clone()),
        ("Characters", join_names(&issue.character_credits)),
        ("Teams", join_names(&issue.team_credits)),
        ("Locations", join_names(&issue.location_credits)),
        ("StoryArc", join_names(&issue.story_arc_credits)),
        ("Notes", format!("Scraped metadata from ComicVine [CVDB{}]", issue.id)),
    ];

    // Cover dates are `YYYY-MM-DD`; the day is nearly always a placeholder `01`.
    if let Some(date) = &issue.cover_date {
        let mut parts = date.split('-');
        values.push(("Year", parts.next().unwrap_or_default().to_owned()));
        values.push(("Month", parts.next().map(strip_leading_zero).unwrap_or_default()));
    }

    for (field, roles) in CREATOR_ROLES {
        values.push((field, creators_with_roles(&issue.person_credits, roles)));
    }

    values.retain(|(_, value)| !value.trim().is_empty());
    values
}

/// ComicInfo creator fields and the ComicVine roles that map to them.
const CREATOR_ROLES: [(&str, &[&str]); 7] = [
    ("Writer", &["writer", "plotter", "scripter"]),
    ("Penciller", &["penciler", "penciller", "artist", "breakdowns"]),
    ("Inker", &["inker", "artist", "finishes"]),
    ("Colorist", &["colorist", "colourist"]),
    ("Letterer", &["letterer"]),
    ("CoverArtist", &["cover"]),
    ("Editor", &["editor"]),
];

fn creators_with_roles(credits: &[Credit], roles: &[&str]) -> String {
    let names: Vec<&str> = credits
        .iter()
        .filter(|credit| {
            credit
                .role
                .split(',')
                .any(|role| roles.contains(&role.trim().to_lowercase().as_str()))
        })
        .map(|credit| credit.name.as_str())
        .collect();
    names.join(", ")
}

fn join_names(credits: &[Credit]) -> String {
    let names: Vec<&str> = credits.iter().map(|c| c.name.as_str()).collect();
    names.join(", ")
}

fn strip_leading_zero(month: &str) -> String {
    month.trim_start_matches('0').to_owned()
}

/// Converts ComicVine's HTML descriptions to plain text. Tables (usually
/// lists of variant covers) are dropped.
pub fn html_to_text(html: &str) -> String {
    let html = truncate_at(html, "<h4>List of covers");
    let html = remove_sections(html, "<table", "</table>");

    let mut text = String::with_capacity(html.len());
    let mut rest = html.as_str();
    loop {
        let Some(start) = rest.find('<') else {
            text.push_str(rest);
            break;
        };
        text.push_str(&rest[..start]);
        // An unterminated tag at the end is dropped.
        let Some(len) = rest[start..].find('>') else { break };
        let tag = rest[start + 1..start + len].trim_end_matches('/').trim().to_ascii_lowercase();
        if LINE_BREAK_TAGS.contains(&tag.as_str()) {
            text.push('\n');
        }
        rest = &rest[start + len + 1..];
    }

    let text = decode_entities(&text);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    collapse_blank_lines(&lines)
}

const LINE_BREAK_TAGS: [&str; 8] = ["br", "/p", "/li", "/h1", "/h2", "/h3", "/h4", "/div"];

/// Everything before the first (case-insensitive) occurrence of `marker`.
fn truncate_at<'a>(html: &'a str, marker: &str) -> &'a str {
    match html.to_ascii_lowercase().find(&marker.to_ascii_lowercase()) {
        Some(pos) => &html[..pos],
        None => html,
    }
}

/// Removes every `start…end` section (case-insensitive). If `end` is never
/// found, everything from `start` onwards is removed.
fn remove_sections(html: &str, start: &str, end: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let (start, end) = (start.to_ascii_lowercase(), end.to_ascii_lowercase());
    let mut out = String::with_capacity(html.len());
    let mut pos = 0;
    while let Some(found) = lower[pos..].find(&start) {
        let begin = pos + found;
        out.push_str(&html[pos..begin]);
        match lower[begin..].find(&end) {
            Some(close) => pos = begin + close + end.len(),
            None => return out,
        }
    }
    out.push_str(&html[pos..]);
    out
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&rsquo;", "’")
        .replace("&lsquo;", "‘")
        .replace("&rdquo;", "”")
        .replace("&ldquo;", "“")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
        .replace("&amp;", "&")
}

fn collapse_blank_lines(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut blank_run = 0;
    for line in lines {
        if line.is_empty() {
            blank_run += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank_run > 0 { "\n\n" } else { "\n" });
        }
        out.push_str(line);
        blank_run = 0;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_plain_text() {
        let html = "<p>Hazel &amp; her <em>parents</em>.</p><p>Second<br/>line</p>\
                    <h4>List of covers and their creators:</h4><table><tr><td>x</td></tr></table>";
        assert_eq!(html_to_text(html), "Hazel & her parents.\nSecond\nline");
    }

    #[test]
    fn maps_issue_fields() {
        let credit = |name: &str, role: &str| Credit { name: name.into(), role: role.into() };
        let volume = Volume {
            id: 1,
            name: "Saga".into(),
            start_year: Some("2012".into()),
            publisher: Some(super::super::api::Named { name: "Image".into() }),
            count_of_issues: 66,
            image: None,
        };
        let issue = Issue {
            id: 42,
            name: Some("Chapter Twelve".into()),
            issue_number: "12".into(),
            cover_date: Some("2013-06-01".into()),
            description: None,
            site_detail_url: "https://comicvine.gamespot.com/x".into(),
            image: None,
            person_credits: vec![credit("BKV", "writer"), credit("Fiona Staples", "artist, cover")],
            character_credits: vec![credit("Hazel", ""), credit("Alana", "")],
            team_credits: vec![],
            location_credits: vec![],
            story_arc_credits: vec![],
        };

        let values = comicinfo_values(&volume, &issue);
        let get = |field| values.iter().find(|(f, _)| *f == field).map(|(_, v)| v.as_str());
        assert_eq!(get("Series"), Some("Saga"));
        assert_eq!(get("Volume"), Some("2012"));
        assert_eq!(get("Month"), Some("6"));
        assert_eq!(get("Writer"), Some("BKV"));
        assert_eq!(get("Penciller"), Some("Fiona Staples"));
        assert_eq!(get("Inker"), Some("Fiona Staples"));
        assert_eq!(get("CoverArtist"), Some("Fiona Staples"));
        assert_eq!(get("Characters"), Some("Hazel, Alana"));
        assert_eq!(get("Teams"), None, "empty values are omitted");
        assert_eq!(get("Notes"), Some("Scraped metadata from ComicVine [CVDB42]"));
    }
}
