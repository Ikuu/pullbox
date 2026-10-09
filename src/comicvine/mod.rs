//! Looking up comic metadata on ComicVine (<https://comicvine.gamespot.com>).

mod api;
mod mapping;
mod matching;
mod worker;

pub use api::{Issue, Volume};
pub use mapping::comicinfo_values;
pub use matching::Lookup;
pub use worker::{Event, Request, Session};

/// Where users get an API key.
pub const API_KEY_URL: &str = "https://comicvine.gamespot.com/api/";
