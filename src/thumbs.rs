//! Cover images from disk, decoded on worker threads into GPU textures kept
//! in an LRU.

use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

const MAX_SIZE: u32 = 720;
const WORKERS: usize = 3;

enum Slot {
    Loading,
    Ready(TextureHandle),
    Failed,
}

pub struct Thumbs {
    ctx: egui::Context,
    cache: LruCache<PathBuf, Slot>,
    requests: flume::Sender<PathBuf>,
    results: flume::Receiver<(PathBuf, Option<ColorImage>)>,
}

fn decode(path: &Path) -> Option<ColorImage> {
    let img = image::open(path).ok()?;
    let img = if img.width() > MAX_SIZE || img.height() > MAX_SIZE {
        img.thumbnail(MAX_SIZE, MAX_SIZE)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

impl Thumbs {
    pub fn new(ctx: egui::Context) -> Self {
        let (requests, req_rx) = flume::unbounded::<PathBuf>();
        let (res_tx, results) = flume::unbounded();
        for _ in 0..WORKERS {
            let rx = req_rx.clone();
            let tx = res_tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                for path in rx.iter() {
                    let img = decode(&path);
                    let _ = tx.send((path, img));
                    ctx.request_repaint();
                }
            });
        }
        Self {
            ctx,
            cache: LruCache::new(NonZeroUsize::new(300).expect("non-zero")),
            requests,
            results,
        }
    }

    pub fn poll(&mut self) {
        for (path, img) in self.results.try_iter() {
            let slot = match img {
                Some(img) => Slot::Ready(self.ctx.load_texture(path.to_string_lossy(), img, TextureOptions::LINEAR)),
                None => Slot::Failed,
            };
            self.cache.put(path, slot);
        }
    }

    pub fn get(&mut self, path: &Path) -> Option<TextureHandle> {
        if !self.cache.contains(path) {
            self.cache.put(path.to_path_buf(), Slot::Loading);
            let _ = self.requests.send(path.to_path_buf());
        }
        match self.cache.get(path)? {
            Slot::Ready(t) => Some(t.clone()),
            _ => None,
        }
    }

}

/// A stable tint per game for covers that are missing.
pub fn placeholder_color(name: &str, p: &crate::theme::M3Palette) -> Color32 {
    let h = name.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    match h % 3 {
        0 => p.primary_container,
        1 => p.secondary_container,
        _ => p.tertiary_container,
    }
}
