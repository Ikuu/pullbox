//! A background thread that searches AniList one request at a time (the API
//! is rate limited) and caches results, so a batch of files from the same
//! series only searches for it once.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use super::api::{Client, Media};
use crate::comicvine::normalize_name;

/// Search AniList for `query`. `target` identifies which file (in the
/// caller's list) the search is for.
pub struct Request {
    pub target: usize,
    pub query: String,
}

pub enum Event {
    Results { target: usize, results: Vec<Media> },
    Failed { target: usize, error: String },
}

/// Connection to the worker thread. Dropping it stops the thread after its
/// current request.
pub struct Session {
    requests: Sender<Request>,
    pub events: Receiver<Event>,
}

impl Session {
    pub fn start(notify: impl Fn() + Send + 'static) -> Self {
        let (requests, request_rx) = channel::<Request>();
        let (event_tx, events) = channel();
        thread::spawn(move || {
            let mut client = Client::new();
            // Normalised query → results.
            let mut searches: HashMap<String, Vec<Media>> = HashMap::new();
            for Request { target, query } in request_rx {
                let key = normalize_name(&query);
                let event = match searches.get(&key) {
                    Some(results) => Event::Results { target, results: results.clone() },
                    None => match client.search(&query) {
                        Ok(results) => {
                            searches.insert(key, results.clone());
                            Event::Results { target, results }
                        }
                        Err(e) => Event::Failed { target, error: format!("{e:#}") },
                    },
                };
                if event_tx.send(event).is_err() {
                    return;
                }
                notify();
            }
        });
        Self { requests, events }
    }

    pub fn send(&self, request: Request) {
        // Only fails if the worker thread has died, in which case no events
        // will arrive and the dialog simply shows the item as still searching.
        let _ = self.requests.send(request);
    }
}
