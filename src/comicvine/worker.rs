//! A background thread that performs ComicVine lookups one at a time (the API
//! is rate limited) and caches results, so a batch of issues from the same
//! series only searches for it once.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use anyhow::Result;

use super::api::{Client, Issue, Volume};
use super::matching::{Lookup, normalize_name, normalize_number, rank_volumes};

/// How many of the best-ranked series to check for the issue number.
const CANDIDATES_TO_TRY: usize = 3;

/// `target` identifies which comic (in the caller's list) a request is for.
pub enum Request {
    /// Search for the comic's series and pick the best matching issue.
    AutoMatch { target: usize, lookup: Lookup },
    /// Search series by name, for the user to choose from.
    Search { target: usize, query: String },
    /// Find the issue in a series the user picked.
    MatchInVolume { target: usize, volume: Volume, number: String },
}

pub enum Event {
    Volumes { target: usize, volumes: Vec<Volume> },
    Matched { target: usize, volume: Volume, issue: Box<Issue> },
    NoMatch { target: usize },
    Failed { target: usize, error: String },
}

/// Connection to the worker thread. Dropping it stops the thread after its
/// current request.
pub struct Session {
    requests: Sender<Request>,
    pub events: Receiver<Event>,
}

impl Session {
    pub fn start(api_key: String, notify: impl Fn() + Send + 'static) -> Self {
        let (requests, request_rx) = channel();
        let (event_tx, events) = channel();
        thread::spawn(move || {
            let mut worker = Worker {
                client: Client::new(api_key),
                searches: HashMap::new(),
                issues: HashMap::new(),
            };
            for request in request_rx {
                let event = worker.handle(request, &|e| {
                    let _ = event_tx.send(e);
                    notify();
                });
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

struct Worker {
    client: Client,
    /// Normalised query → results.
    searches: HashMap<String, Vec<Volume>>,
    /// (volume id, issue number) → issue, if it exists.
    issues: HashMap<(u64, String), Option<Issue>>,
}

impl Worker {
    /// Handles one request. `emit` sends intermediate events; the returned
    /// event is the final one.
    fn handle(&mut self, request: Request, emit: &dyn Fn(Event)) -> Event {
        let target = match &request {
            Request::AutoMatch { target, .. } | Request::Search { target, .. } | Request::MatchInVolume { target, .. } => {
                *target
            }
        };
        self.try_handle(request, emit)
            .unwrap_or_else(|e| Event::Failed { target, error: format!("{e:#}") })
    }

    fn try_handle(&mut self, request: Request, emit: &dyn Fn(Event)) -> Result<Event> {
        Ok(match request {
            Request::Search { target, query } => Event::Volumes { target, volumes: self.search(&query)? },
            Request::MatchInVolume { target, volume, number } => match self.issue(volume.id, &number)? {
                Some(issue) => Event::Matched { target, volume, issue: Box::new(issue) },
                None => Event::NoMatch { target },
            },
            Request::AutoMatch { target, lookup } => {
                let volumes = self.search(&lookup.series)?;
                emit(Event::Volumes { target, volumes: volumes.clone() });
                if lookup.number.is_empty() {
                    return Ok(Event::NoMatch { target });
                }
                for volume in rank_volumes(&volumes, &lookup).into_iter().take(CANDIDATES_TO_TRY) {
                    if let Some(issue) = self.issue(volume.id, &lookup.number)? {
                        return Ok(Event::Matched { target, volume: volume.clone(), issue: Box::new(issue) });
                    }
                }
                Event::NoMatch { target }
            }
        })
    }

    fn search(&mut self, query: &str) -> Result<Vec<Volume>> {
        let key = normalize_name(query);
        if let Some(cached) = self.searches.get(&key) {
            return Ok(cached.clone());
        }
        let volumes = self.client.search_volumes(query)?;
        self.searches.insert(key, volumes.clone());
        Ok(volumes)
    }

    fn issue(&mut self, volume_id: u64, number: &str) -> Result<Option<Issue>> {
        let key = (volume_id, normalize_number(number));
        if let Some(cached) = self.issues.get(&key) {
            return Ok(cached.clone());
        }
        let issue = self.client.find_issue(volume_id, &key.1)?;
        self.issues.insert(key, issue.clone());
        Ok(issue)
    }
}
