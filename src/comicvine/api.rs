//! Minimal blocking client for the ComicVine REST API.
//! See <https://comicvine.gamespot.com/api/documentation>.

use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;

const BASE_URL: &str = "https://comicvine.gamespot.com/api";
/// ComicVine rejects requests without a descriptive User-Agent.
const USER_AGENT: &str = concat!("pullbox/", env!("CARGO_PKG_VERSION"));
/// ComicVine throttles clients making more than about one request per second.
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(1100);

const VOLUME_FIELDS: &str = "id,name,start_year,publisher,count_of_issues,image";
const ISSUE_LIST_FIELDS: &str = "id";
const ISSUE_DETAIL_FIELDS: &str = "id,name,issue_number,cover_date,description,site_detail_url,image,volume,\
     person_credits,character_credits,team_credits,location_credits,story_arc_credits";

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Image {
    #[serde(default)]
    pub small_url: String,
}

/// A series ("volume" in ComicVine terms).
#[derive(Clone, Debug, Deserialize)]
pub struct Volume {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    pub start_year: Option<String>,
    pub publisher: Option<Named>,
    #[serde(default)]
    pub count_of_issues: u32,
    pub image: Option<Image>,
}

impl Volume {
    pub fn start_year(&self) -> Option<i32> {
        self.start_year.as_deref()?.trim().parse().ok()
    }

    pub fn publisher_name(&self) -> &str {
        self.publisher.as_ref().map_or("", |p| p.name.as_str())
    }

    /// Whether the series' publisher is one of `ignored` (ignoring case).
    pub fn publisher_ignored(&self, ignored: &[String]) -> bool {
        let name = self.publisher_name();
        !name.is_empty() && ignored.iter().any(|i| i.to_lowercase() == name.to_lowercase())
    }
}

#[derive(Clone, Debug, Deserialize)]
struct IssueSummary {
    id: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Credit {
    #[serde(default)]
    pub name: String,
    /// Comma-separated, e.g. `"writer, cover"`. Only present on person credits.
    #[serde(default)]
    pub role: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Issue {
    pub id: u64,
    pub name: Option<String>,
    #[serde(default)]
    pub issue_number: String,
    /// `YYYY-MM-DD`.
    pub cover_date: Option<String>,
    /// HTML.
    pub description: Option<String>,
    #[serde(default)]
    pub site_detail_url: String,
    pub image: Option<Image>,
    #[serde(default)]
    pub person_credits: Vec<Credit>,
    #[serde(default)]
    pub character_credits: Vec<Credit>,
    #[serde(default)]
    pub team_credits: Vec<Credit>,
    #[serde(default)]
    pub location_credits: Vec<Credit>,
    #[serde(default)]
    pub story_arc_credits: Vec<Credit>,
}

#[derive(Deserialize)]
struct Envelope<T> {
    status_code: u32,
    error: String,
    results: Option<T>,
}

pub struct Client {
    agent: ureq::Agent,
    api_key: String,
    last_request: Option<Instant>,
}

impl Client {
    pub fn new(api_key: String) -> Self {
        let config = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(30)))
            // Error statuses are handled below, so their messages can be specific.
            .http_status_as_error(false)
            .build();
        Self { agent: ureq::Agent::new_with_config(config), api_key, last_request: None }
    }

    /// Searches series by name, best matches first (as ranked by ComicVine).
    pub fn search_volumes(&mut self, query: &str) -> Result<Vec<Volume>> {
        self.get(
            "/search/",
            &[
                ("query", query),
                ("resources", "volume"),
                ("field_list", VOLUME_FIELDS),
                ("limit", "30"),
            ],
        )
    }

    /// Finds an issue by number within a volume and returns its full details.
    pub fn find_issue(&mut self, volume_id: u64, issue_number: &str) -> Result<Option<Issue>> {
        let filter = format!("volume:{volume_id},issue_number:{issue_number}");
        let found: Vec<IssueSummary> =
            self.get("/issues/", &[("filter", &filter), ("field_list", ISSUE_LIST_FIELDS)])?;
        match found.first() {
            Some(summary) => self.issue(summary.id).map(Some),
            None => Ok(None),
        }
    }

    pub fn issue(&mut self, id: u64) -> Result<Issue> {
        self.get(&format!("/issue/4000-{id}/"), &[("field_list", ISSUE_DETAIL_FIELDS)])
    }

    fn get<T: DeserializeOwned>(&mut self, path: &str, params: &[(&str, &str)]) -> Result<T> {
        self.throttle();

        let mut request = self
            .agent
            .get(format!("{BASE_URL}{path}"))
            .query("api_key", &self.api_key)
            .query("format", "json");
        for (key, value) in params {
            request = request.query(*key, *value);
        }

        let mut response = request.call().context("could not reach ComicVine")?;
        match response.status().as_u16() {
            200 => {}
            401 => bail!("invalid ComicVine API key"),
            420 | 429 => bail!("ComicVine rate limit exceeded; try again later"),
            status => bail!("ComicVine returned HTTP {status}"),
        }
        let envelope: Envelope<T> = response
            .body_mut()
            .read_json()
            .context("unexpected response from ComicVine")?;

        match envelope.status_code {
            1 => envelope.results.context("ComicVine returned no results"),
            100 => bail!("invalid ComicVine API key"),
            107 => bail!("ComicVine rate limit exceeded; try again later"),
            _ => bail!("ComicVine error: {}", envelope.error),
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
    #[ignore = "contacts comicvine.gamespot.com; run with --ignored"]
    fn invalid_key_is_reported() {
        let error = Client::new("not-a-real-key".into()).search_volumes("Saga").unwrap_err();
        assert!(format!("{error:#}").contains("invalid ComicVine API key"), "{error:#}");
    }
}
