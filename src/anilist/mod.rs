//! Looking up manga on AniList (<https://anilist.co>). Its API is free and
//! needs no key, but only knows about whole series, not volumes or chapters.

mod api;
mod mapping;
mod matching;
mod worker;

pub use api::Media;
pub use mapping::comicinfo_values;
pub use matching::{MangaLookup, best_match};
pub use worker::{Event, Request, Session};
