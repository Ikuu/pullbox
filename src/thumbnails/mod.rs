//! Cover thumbnails, loaded on background threads and uploaded as textures.
//!
//! Requests are served most-recent-first, so whatever is on screen right now
//! loads before items the user has already scrolled past. Only a bounded
//! number of textures is kept in GPU memory; evicted ones reload quickly from
//! the disk cache.

mod generate;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use generate::ThumbKey;

use crate::library::ComicRecord;

const WORKER_THREADS: usize = 4;
const MAX_TEXTURES: usize = 400;

enum Slot {
    Pending,
    Ready { texture: egui::TextureHandle, last_used: u64 },
    Failed,
}

/// Thumbnails waiting to be generated, newest last.
#[derive(Default)]
struct RequestStack {
    keys: Mutex<Vec<ThumbKey>>,
    available: Condvar,
}

pub struct Thumbnails {
    requests: Arc<RequestStack>,
    results: Receiver<(ThumbKey, Option<egui::ColorImage>)>,
    slots: HashMap<ThumbKey, Slot>,
    frame: u64,
}

impl Thumbnails {
    pub fn new(ctx: &egui::Context) -> Self {
        let cache_dir = crate::util::project_dirs()
            .map(|dirs| dirs.cache_dir().join("thumbnails"))
            .unwrap_or_else(|| std::env::temp_dir().join("comic-editor-thumbnails"));
        let requests = Arc::new(RequestStack::default());
        let (tx, results) = channel();

        for _ in 0..WORKER_THREADS {
            let worker = Worker {
                requests: Arc::clone(&requests),
                results: tx.clone(),
                cache_dir: cache_dir.clone(),
                ctx: ctx.clone(),
            };
            thread::spawn(move || worker.run());
        }

        Self { requests, results, slots: HashMap::new(), frame: 0 }
    }

    /// Call once per frame before any `get`: uploads finished thumbnails and
    /// evicts textures that haven't been drawn recently.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        while let Ok((key, image)) = self.results.try_recv() {
            let slot = match image {
                Some(image) => {
                    let name = key.path.to_string_lossy();
                    let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
                    Slot::Ready { texture, last_used: self.frame }
                }
                None => Slot::Failed,
            };
            self.slots.insert(key, slot);
        }
        self.evict();
    }

    /// The comic's cover thumbnail, or `None` while it loads (or if it can't).
    pub fn get(&mut self, comic: &ComicRecord) -> Option<&egui::TextureHandle> {
        let key = ThumbKey {
            path: comic.path.clone(),
            size: comic.size,
            modified: comic.modified,
        };
        let slot = self.slots.entry(key).or_insert_with_key(|key| {
            self.requests.push(key.clone());
            Slot::Pending
        });
        match slot {
            Slot::Ready { texture, last_used } => {
                *last_used = self.frame;
                Some(texture)
            }
            Slot::Pending | Slot::Failed => None,
        }
    }

    /// Drops the least recently drawn textures beyond `MAX_TEXTURES`.
    fn evict(&mut self) {
        let loaded = self.slots.values().filter(|s| matches!(s, Slot::Ready { .. })).count();
        let excess = loaded.saturating_sub(MAX_TEXTURES);
        if excess == 0 {
            return;
        }

        let mut candidates: Vec<(u64, ThumbKey)> = self
            .slots
            .iter()
            .filter_map(|(key, slot)| match slot {
                Slot::Ready { last_used, .. } => Some((*last_used, key.clone())),
                _ => None,
            })
            .collect();
        candidates.sort_unstable_by_key(|(last_used, _)| *last_used);
        for (_, key) in candidates.into_iter().take(excess) {
            self.slots.remove(&key);
        }
    }
}

impl RequestStack {
    fn push(&self, key: ThumbKey) {
        self.keys.lock().unwrap().push(key);
        self.available.notify_one();
    }

    /// Blocks until a request is available, then takes the newest.
    fn pop(&self) -> ThumbKey {
        let mut keys = self.keys.lock().unwrap();
        loop {
            if let Some(key) = keys.pop() {
                return key;
            }
            keys = self.available.wait(keys).unwrap();
        }
    }
}

struct Worker {
    requests: Arc<RequestStack>,
    results: Sender<(ThumbKey, Option<egui::ColorImage>)>,
    cache_dir: PathBuf,
    ctx: egui::Context,
}

impl Worker {
    fn run(self) {
        loop {
            let key = self.requests.pop();
            let image = match generate::load_or_generate(&key, &self.cache_dir) {
                Ok(rgba) => {
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    Some(egui::ColorImage::from_rgba_unmultiplied(size, &rgba))
                }
                Err(e) => {
                    eprintln!("thumbnail: {}: {e:#}", key.path.display());
                    None
                }
            };
            if self.results.send((key, image)).is_err() {
                return; // The app has shut down.
            }
            self.ctx.request_repaint();
        }
    }
}
