//! Minimal blocking client for the AniList GraphQL API.
//! See <https://docs.anilist.co>.

use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

const URL: &str = "https://graphql.anilist.co";
const USER_AGENT: &str = concat!("comic-editor/", env!("CARGO_PKG_VERSION"));
/// AniList normally allows 90 requests a minute, but cuts that to 30 while
/// its API is degraded (as it has been for a while), so keep under that.
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(2100);
/// Longest `Retry-After` worth waiting for; a rate limit lasts a minute.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(65);

const SEARCH_QUERY: &str = "query ($search: String) {
  Page(perPage: 15) {
    media(search: $search, type: MANGA, sort: SEARCH_MATCH) {
      id
      title { romaji english native }
      synonyms
      format
      countryOfOrigin
      startDate { year month }
      volumes
      chapters
      genres
      tags { name rank isMediaSpoiler }
      description(asHtml: false)
      siteUrl
      coverImage { medium large }
      averageScore
      isAdult
      staff(sort: [RELEVANCE], perPage: 25) { edges { role node { name { full } } } }
    }
  }
}";

/// A manga (or manhwa, light novel, …) on AniList.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: u64,
    #[serde(default, deserialize_with = "nullable")]
    pub title: Title,
    #[serde(default, deserialize_with = "nullable")]
    pub synonyms: Vec<String>,
    /// `MANGA`, `NOVEL` or `ONE_SHOT`.
    #[serde(default, deserialize_with = "nullable")]
    pub format: String,
    /// ISO 3166 code: `JP`, `KR`, `CN`, `TW`, …
    #[serde(default, deserialize_with = "nullable")]
    pub country_of_origin: String,
    #[serde(default, deserialize_with = "nullable")]
    pub start_date: FuzzyDate,
    /// Only known once the series has finished.
    pub volumes: Option<u32>,
    pub chapters: Option<u32>,
    #[serde(default, deserialize_with = "nullable")]
    pub genres: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub tags: Vec<Tag>,
    /// Mostly plain text, with some HTML (`<br>`, `<i>`, links).
    #[serde(default, deserialize_with = "nullable")]
    pub description: String,
    #[serde(default, deserialize_with = "nullable")]
    pub site_url: String,
    #[serde(default, deserialize_with = "nullable")]
    pub cover_image: CoverImage,
    /// Out of 100.
    pub average_score: Option<u32>,
    #[serde(default, deserialize_with = "nullable")]
    pub is_adult: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub staff: StaffConnection,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Title {
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FuzzyDate {
    pub year: Option<i32>,
    pub month: Option<i32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CoverImage {
    pub medium: Option<String>,
    pub large: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    /// How relevant the tag is, out of 100.
    #[serde(default, deserialize_with = "nullable")]
    pub rank: u32,
    #[serde(default, deserialize_with = "nullable")]
    pub is_media_spoiler: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct StaffConnection {
    #[serde(default, deserialize_with = "nullable")]
    pub edges: Vec<StaffEdge>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct StaffEdge {
    /// e.g. `Story & Art`, `Art (vols 1-3)`, `Translator (English)`.
    #[serde(default, deserialize_with = "nullable")]
    pub role: String,
    #[serde(default, deserialize_with = "nullable")]
    pub node: StaffNode,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct StaffNode {
    #[serde(default, deserialize_with = "nullable")]
    pub name: StaffName,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct StaffName {
    pub full: Option<String>,
}

impl Media {
    /// The English title if there is one, otherwise the romanised one.
    pub fn display_title(&self) -> &str {
        let t = &self.title;
        t.english.as_deref().or(t.romaji.as_deref()).or(t.native.as_deref()).unwrap_or_default()
    }

    /// Every name the series goes by.
    pub fn titles(&self) -> impl Iterator<Item = &str> {
        let t = &self.title;
        [&t.romaji, &t.english, &t.native]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .chain(self.synonyms.iter().map(String::as_str))
    }

    pub fn cover_url(&self) -> Option<&str> {
        let cover = &self.cover_image;
        cover.large.as_deref().or(cover.medium.as_deref())
    }

    /// "Manga", "Manhwa", "Light Novel", …
    pub fn kind_label(&self) -> &'static str {
        match (self.format.as_str(), self.country_of_origin.as_str()) {
            ("NOVEL", _) => "Light Novel",
            ("ONE_SHOT", _) => "One-shot",
            (_, "KR") => "Manhwa",
            (_, "CN" | "TW") => "Manhua",
            _ => "Manga",
        }
    }
}

/// Reads `null` as the type's default, as AniList sends nulls freely.
fn nullable<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(deserializer: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Deserialize)]
struct Response<T> {
    data: Option<T>,
    #[serde(default, deserialize_with = "nullable")]
    errors: Vec<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default, deserialize_with = "nullable")]
    message: String,
}

#[derive(Deserialize)]
struct SearchData {
    #[serde(rename = "Page")]
    page: SearchPage,
}

#[derive(Deserialize)]
struct SearchPage {
    #[serde(default, deserialize_with = "nullable")]
    media: Vec<Media>,
}

pub struct Client {
    agent: ureq::Agent,
    last_request: Option<Instant>,
}

impl Client {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(30)))
            // Error statuses are handled below, so their messages can be specific.
            .http_status_as_error(false)
            .build();
        Self { agent: ureq::Agent::new_with_config(config), last_request: None }
    }

    /// Searches manga by title, best matches first (as ranked by AniList).
    pub fn search(&mut self, query: &str) -> Result<Vec<Media>> {
        let data: SearchData = self.post(SEARCH_QUERY, serde_json::json!({ "search": query }))?;
        Ok(data.page.media)
    }

    fn post<T: DeserializeOwned>(&mut self, query: &str, variables: serde_json::Value) -> Result<T> {
        let body = serde_json::json!({ "query": query, "variables": variables });
        let mut retried = false;
        loop {
            self.throttle();
            let mut response = self
                .agent
                .post(URL)
                .header("Accept", "application/json")
                .send_json(&body)
                .context("could not reach AniList")?;

            let status = response.status().as_u16();
            if status == 429 {
                let wait = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()?.trim().parse().ok())
                    .map(Duration::from_secs);
                match wait {
                    Some(wait) if !retried && wait <= MAX_RETRY_WAIT => {
                        thread::sleep(wait);
                        retried = true;
                        continue;
                    }
                    _ => bail!("AniList rate limit exceeded; try again in a minute"),
                }
            }

            // GraphQL errors come with a JSON body explaining them, whatever the status.
            let parsed: Option<Response<T>> = response.body_mut().read_json().ok();
            let message = parsed.as_ref().and_then(|r| r.errors.first()).map(|e| e.message.clone());
            return match (status, parsed, message) {
                (200, Some(Response { data: Some(data), .. }), _) => Ok(data),
                (_, _, Some(message)) => bail!("AniList error: {message}"),
                (500.., _, None) => bail!("AniList is unavailable (HTTP {status}); try again later"),
                (200, _, None) => bail!("unexpected response from AniList"),
                (_, _, None) => bail!("AniList returned HTTP {status}"),
            };
        }
    }

    /// Sleeps so requests are at least `MIN_REQUEST_INTERVAL` apart.
    fn throttle(&mut self) {
        if let Some(last) = self.last_request {
            let elapsed = last.elapsed();
            if elapsed < MIN_REQUEST_INTERVAL {
                thread::sleep(MIN_REQUEST_INTERVAL - elapsed);
            }
        }
        self.last_request = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_media_with_nulls() {
        let json = r#"{"id": 1, "title": {"romaji": "Berserk", "english": null, "native": null},
            "synonyms": null, "format": "MANGA", "countryOfOrigin": "JP", "startDate": {"year": 1989, "month": null},
            "volumes": null, "chapters": null, "genres": ["Action"], "tags": null, "description": null,
            "siteUrl": "https://anilist.co/manga/1", "coverImage": null, "averageScore": 92, "isAdult": false,
            "staff": {"edges": [{"role": "Story & Art", "node": {"name": {"full": "Kentarou Miura"}}}]}}"#;
        let media: Media = serde_json::from_str(json).unwrap();
        assert_eq!(media.display_title(), "Berserk");
        assert_eq!(media.start_date.year, Some(1989));
        assert!(media.synonyms.is_empty() && media.tags.is_empty() && media.description.is_empty());
        assert_eq!(media.staff.edges[0].node.name.full.as_deref(), Some("Kentarou Miura"));
    }

    #[test]
    #[ignore = "contacts graphql.anilist.co; run with --ignored"]
    fn finds_a_well_known_series() {
        let results = Client::new().search("Berserk").unwrap();
        assert!(results.iter().any(|m| m.id == 30002), "{results:?}");
    }
}
