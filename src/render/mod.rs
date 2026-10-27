//! Background rendering. Each worker owns its own MuPDF `Document`; the UI
//! thread only replaces the job queue and receives finished images.
//!
//! The queue is a priority list that the UI replaces wholesale whenever the
//! wanted set changes (visible pages first, then prefetch), so jobs for pages
//! that scrolled away are dropped before they start.

pub mod cache;
pub mod tiles;

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crossbeam_channel::{Receiver, Sender};
use eframe::egui::ColorImage;

use crate::document::{self, PixelRect};
use tiles::TileId;

/// A render scale (device pixels per PDF point) rounded to hundredths, so
/// nearly identical scales share cache entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScaleKey(u32);

impl ScaleKey {
    pub fn from_scale(scale: f32) -> Self {
        Self((scale * 100.0).round().max(1.0) as u32)
    }

    pub fn scale(self) -> f32 {
        self.0 as f32 / 100.0
    }

    pub fn distance(self, other: Self) -> u32 {
        self.0.abs_diff(other.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderJob {
    pub page: usize,
    pub scale: ScaleKey,
    pub tile: TileId,
    /// Pixel region of the tile (derived from the fields above).
    pub rect: PixelRect,
}

pub struct RenderResult {
    pub job: RenderJob,
    /// Converted to an egui image on the worker so the UI thread only uploads.
    pub image: Result<ColorImage, String>,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<RenderJob>,
    in_progress: HashSet<RenderJob>,
    shutdown: bool,
}

impl Queue {
    /// Replace pending jobs, skipping ones a worker is already rendering.
    fn replace(&mut self, jobs: impl IntoIterator<Item = RenderJob>) {
        self.jobs.clear();
        let mut seen = HashSet::new();
        for job in jobs {
            if !self.in_progress.contains(&job) && seen.insert(job) {
                self.jobs.push_back(job);
            }
        }
    }

    fn pop(&mut self) -> Option<RenderJob> {
        let job = self.jobs.pop_front()?;
        self.in_progress.insert(job);
        Some(job)
    }
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

pub struct Renderer {
    shared: Arc<Shared>,
    results: Receiver<RenderResult>,
}

impl Renderer {
    /// Spawn `workers` render threads for `path`. `wake` is called after each
    /// finished job so the UI can repaint.
    pub fn spawn(path: PathBuf, workers: usize, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            ready: Condvar::new(),
        });
        let (tx, rx) = crossbeam_channel::unbounded();
        let wake = Arc::new(wake);

        for i in 0..workers.max(1) {
            let shared = Arc::clone(&shared);
            let tx = tx.clone();
            let wake = Arc::clone(&wake);
            let path = path.clone();
            thread::Builder::new()
                .name(format!("pdfv-render-{i}"))
                .spawn(move || worker(path, shared, tx, move || wake()))
                .expect("failed to spawn render thread");
        }

        Self {
            shared,
            results: rx,
        }
    }

    /// Set the wanted jobs, highest priority first.
    pub fn set_queue(&self, jobs: impl IntoIterator<Item = RenderJob>) {
        let mut q = self.shared.queue.lock().unwrap();
        q.replace(jobs);
        if !q.jobs.is_empty() {
            self.shared.ready.notify_all();
        }
    }

    pub fn try_recv(&self) -> Option<RenderResult> {
        self.results.try_recv().ok()
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.shared.queue.lock().unwrap().shutdown = true;
        self.shared.ready.notify_all();
    }
}

fn worker(path: PathBuf, shared: Arc<Shared>, results: Sender<RenderResult>, wake: impl Fn()) {
    let mut renderer = match document::PageRenderer::open(&path) {
        Ok(r) => r,
        Err(err) => {
            tracing::error!("render worker cannot open document: {err:#}");
            return;
        }
    };

    loop {
        let job = {
            let mut q = shared.queue.lock().unwrap();
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(job) = q.pop() {
                    break job;
                }
                q = shared.ready.wait(q).unwrap();
            }
        };

        let start = std::time::Instant::now();
        let image = renderer
            .render(job.page, job.scale.scale(), job.rect)
            .map(|p| ColorImage::from_rgb([p.width, p.height], &p.rgb))
            .map_err(|e| format!("{e:#}"));
        tracing::debug!(
            page = job.page,
            scale = job.scale.scale(),
            col = job.tile.col,
            row = job.tile.row,
            ms = start.elapsed().as_millis() as u64,
            "rendered"
        );

        // Send before clearing `in_progress`, so the UI never sees the job as
        // neither in progress nor delivered (which would re-queue it).
        let sent = results.send(RenderResult { job, image }).is_ok();
        shared.queue.lock().unwrap().in_progress.remove(&job);
        if !sent {
            return;
        }
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(page: usize) -> RenderJob {
        RenderJob {
            page,
            scale: ScaleKey::from_scale(2.0),
            tile: TileId { col: 0, row: 0 },
            rect: PixelRect {
                x: 0,
                y: 0,
                w: 10,
                h: 10,
            },
        }
    }

    #[test]
    fn scale_key_rounds_to_hundredths() {
        assert_eq!(ScaleKey::from_scale(2.0049), ScaleKey::from_scale(2.0));
        assert_eq!(ScaleKey::from_scale(2.006).scale(), 2.01);
    }

    #[test]
    fn replace_drops_stale_jobs_and_skips_in_progress() {
        let mut q = Queue::default();
        q.replace([job(1), job(2), job(3)]);
        assert_eq!(q.pop(), Some(job(1)));

        q.replace([job(1), job(5), job(5), job(4)]);
        assert_eq!(q.pop(), Some(job(5)));
        assert_eq!(q.pop(), Some(job(4)));
        assert_eq!(q.pop(), None);
    }
}
