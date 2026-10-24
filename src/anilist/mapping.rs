//! Turning an AniList series into ComicInfo.xml field values.

use super::api::Media;
use super::matching::MangaLookup;
use crate::comicvine::{html_to_text, normalize_name};

/// Tags AniList is less sure of than this (out of 100) are left out.
const MIN_TAG_RANK: u32 = 60;
const MAX_TAGS: usize = 10;

/// `(ComicInfo element, value)` for every field AniList has a value for, for
/// the file described by `lookup`. AniList only knows about whole series, so
/// the file's volume and chapter come from `lookup`. Fields AniList knows
/// nothing about are left out, so they aren't cleared.
pub fn comicinfo_values(media: &Media, lookup: &MangaLookup) -> Vec<(&'static str, String)> {
    let (series, localized) = series_titles(media, &lookup.series);
    let is_chapter = !lookup.chapter.trim().is_empty();
    let count = if is_chapter { media.chapters } else { media.volumes };
    let tags: Vec<&str> = media
        .tags
        .iter()
        .filter(|t| !t.is_media_spoiler && t.rank >= MIN_TAG_RANK)
        .take(MAX_TAGS)
        .map(|t| t.name.as_str())
        .collect();

    let mut values = vec![
        ("Series", series),
        ("LocalizedSeries", localized),
        ("Number", lookup.chapter.trim().to_owned()),
        ("Volume", lookup.volume.map(|v| v.to_string()).unwrap_or_default()),
        ("Count", count.map(|c| c.to_string()).unwrap_or_default()),
        ("Writer", staff_with_roles(media, &["Story & Art", "Story", "Original Story", "Original Creator"])),
        ("Penciller", staff_with_roles(media, &["Story & Art", "Art"])),
        ("Inker", staff_with_roles(media, &["Story & Art", "Art"])),
        ("Genre", media.genres.join(", ")),
        ("Tags", tags.join(", ")),
        ("Summary", html_to_text(&media.description)),
        ("Web", media.site_url.clone()),
        ("Manga", if media.country_of_origin == "JP" { "YesAndRightToLeft" } else { "Yes" }.to_owned()),
        ("Notes", format!("Scraped metadata from AniList [AniList{}]", media.id)),
    ];
    if media.is_adult {
        values.push(("AgeRating", "Adults Only 18+".to_owned()));
    }
    if let Some(score) = media.average_score {
        values.push(("CommunityRating", format!("{:.1}", score as f32 / 20.0)));
    }
    // A series' start date is only the publication date of a one-shot.
    if media.format == "ONE_SHOT" {
        values.push(("Year", media.start_date.year.map(|y| y.to_string()).unwrap_or_default()));
        values.push(("Month", media.start_date.month.map(|m| m.to_string()).unwrap_or_default()));
    }

    values.retain(|(_, value)| !value.trim().is_empty());
    values
}

/// The series name to use and another title it's known by. The name the
/// files already use is kept if it's one of the series' titles; otherwise
/// the English title is preferred.
fn series_titles(media: &Media, current: &str) -> (String, String) {
    let wanted = normalize_name(current);
    let series = media
        .titles()
        .find(|t| normalize_name(t) == wanted)
        .unwrap_or(media.display_title());
    let series_key = normalize_name(series);
    let localized = [&media.title.english, &media.title.romaji]
        .into_iter()
        .flatten()
        .find(|t| normalize_name(t) != series_key)
        .cloned()
        .unwrap_or_default();
    (series.to_owned(), localized)
}

/// Staff whose role, ignoring details like "(vols 1-3)", is one of `roles`.
fn staff_with_roles(media: &Media, roles: &[&str]) -> String {
    let mut names: Vec<&str> = Vec::new();
    for edge in &media.staff.edges {
        let role = edge.role.split(" (").next().unwrap_or_default().trim();
        if let Some(name) = edge.node.name.full.as_deref()
            && roles.iter().any(|r| r.eq_ignore_ascii_case(role))
            && !names.contains(&name)
        {
            names.push(name);
        }
    }
    names.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anilist::api::{StaffConnection, StaffEdge, StaffName, StaffNode, Tag, Title};

    fn attack_on_titan() -> Media {
        let staff = |role: &str, name: &str| StaffEdge {
            role: role.into(),
            node: StaffNode { name: StaffName { full: Some(name.into()) } },
        };
        let tag = |name: &str, rank, spoiler| Tag { name: name.into(), rank, is_media_spoiler: spoiler };
        Media {
            id: 30002,
            title: Title {
                romaji: Some("Shingeki no Kyojin".into()),
                english: Some("Attack on Titan".into()),
                native: None,
            },
            format: "MANGA".into(),
            country_of_origin: "JP".into(),
            volumes: Some(34),
            chapters: Some(139),
            genres: vec!["Action".into(), "Drama".into()],
            tags: vec![tag("Military", 90, false), tag("Twist", 80, true), tag("Gore", 40, false)],
            description: "Humanity <i>fights</i>.<br>\n<br>\n(Source: Kodansha)".into(),
            site_url: "https://anilist.co/manga/53390".into(),
            average_score: Some(84),
            staff: StaffConnection {
                edges: vec![
                    staff("Story & Art", "Hajime Isayama"),
                    staff("Story & Art (vols 1-2)", "Hajime Isayama"),
                    staff("Translator (English)", "Sheldon Drzka"),
                ],
            },
            ..Default::default()
        }
    }

    fn values(lookup: &MangaLookup) -> Vec<(&'static str, String)> {
        comicinfo_values(&attack_on_titan(), lookup)
    }

    fn get<'a>(values: &'a [(&'static str, String)], field: &str) -> Option<&'a str> {
        values.iter().find(|(f, _)| *f == field).map(|(_, v)| v.as_str())
    }

    #[test]
    fn maps_a_volume() {
        let lookup = MangaLookup { series: "shingeki no kyojin".into(), volume: Some(3), ..Default::default() };
        let values = values(&lookup);
        assert_eq!(get(&values, "Series"), Some("Shingeki no Kyojin"), "keeps the name already in use");
        assert_eq!(get(&values, "LocalizedSeries"), Some("Attack on Titan"));
        assert_eq!(get(&values, "Volume"), Some("3"));
        assert_eq!(get(&values, "Number"), None);
        assert_eq!(get(&values, "Count"), Some("34"));
        assert_eq!(get(&values, "Writer"), Some("Hajime Isayama"));
        assert_eq!(get(&values, "Penciller"), Some("Hajime Isayama"));
        assert_eq!(get(&values, "Tags"), Some("Military"));
        assert_eq!(get(&values, "Summary"), Some("Humanity fights.\n\n(Source: Kodansha)"));
        assert_eq!(get(&values, "Manga"), Some("YesAndRightToLeft"));
        assert_eq!(get(&values, "CommunityRating"), Some("4.2"));
        assert_eq!(get(&values, "Year"), None);
    }

    #[test]
    fn maps_a_chapter() {
        let lookup = MangaLookup { series: "AoT".into(), chapter: "97".into(), ..Default::default() };
        let values = values(&lookup);
        assert_eq!(get(&values, "Series"), Some("Attack on Titan"), "English when the name isn't a title");
        assert_eq!(get(&values, "LocalizedSeries"), Some("Shingeki no Kyojin"));
        assert_eq!(get(&values, "Number"), Some("97"));
        assert_eq!(get(&values, "Count"), Some("139"));
    }
}
