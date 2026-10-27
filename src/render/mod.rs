//! Background rendering. The worker owns its own MuPDF `Document`; the UI
//! thread only sends jobs and receives pixel buffers.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::document::{self, RenderedPage};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderJob {
    pub page: usize,
    /// Device pixels per PDF point.
    pub scale: f32,
}

#[derive(Debug)]
pub struct RenderResult {
    pub job: RenderJob,
    pub image: Result<RenderedPage, String>,
}

pub struct Renderer {
    jobs: Sender<RenderJob>,
    results: Receiver<RenderResult>,
}

impl Renderer {
    /// Spawn a worker for `path`. `wake` is called after each finished job so
    /// the UI can repaint.
    pub fn spawn(path: PathBuf, wake: impl Fn() + Send + 'static) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<RenderJob>();
        let (res_tx, res_rx) = mpsc::channel::<RenderResult>();

        thread::Builder::new()
            .name("pdfv-render".into())
            .spawn(move || worker(path, job_rx, res_tx, wake))
            .expect("failed to spawn render thread");

        Self {
            jobs: job_tx,
            results: res_rx,
        }
    }

    pub fn request(&self, job: RenderJob) {
        // The worker only exits when this sender is dropped, so this can't fail
        // while `self` is alive unless the worker panicked.
        if self.jobs.send(job).is_err() {
            tracing::error!("render worker is gone");
        }
    }

    pub fn try_recv(&self) -> Option<RenderResult> {
        self.results.try_recv().ok()
    }
}

fn worker(
    path: PathBuf,
    jobs: Receiver<RenderJob>,
    results: Sender<RenderResult>,
    wake: impl Fn(),
) {
    let doc = match document::open(&path) {
        Ok(doc) => doc,
        Err(err) => {
            tracing::error!("render worker cannot open document: {err:#}");
            return;
        }
    };

    while let Ok(mut job) = jobs.recv() {
        // Coalesce: if newer jobs queued up (e.g. during a resize), only the
        // latest one matters.
        while let Ok(newer) = jobs.try_recv() {
            job = newer;
        }

        let start = std::time::Instant::now();
        let image = document::render_page(&doc, job.page, job.scale).map_err(|e| format!("{e:#}"));
        tracing::debug!(
            page = job.page,
            scale = job.scale,
            ms = start.elapsed().as_millis() as u64,
            "rendered"
        );

        if results.send(RenderResult { job, image }).is_err() {
            break;
        }
        wake();
    }
}
