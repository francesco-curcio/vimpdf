//! Link previews: a thread that renders a strip of a page (where a link
//! leads) for the hover popup, so the UI never waits for MuPDF.

use std::path::PathBuf;
use std::thread;

use crossbeam_channel::{Receiver, Sender};
use eframe::egui::ColorImage;

use super::{Recolor, recolor};
use crate::document::{PageRenderer, PixelRect};

/// A strip of a page: full width, from `top` down `height` points.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewRequest {
    /// The file to render (the working copy while annotations are unsaved).
    pub source: PathBuf,
    pub page: usize,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    /// Device pixels per PDF point.
    pub scale: f32,
    pub recolor: Option<Recolor>,
}

impl PreviewRequest {
    fn region(&self) -> PixelRect {
        PixelRect {
            x: 0,
            y: (self.top * self.scale).floor().max(0.0) as u32,
            w: ((self.width * self.scale).ceil() as u32).max(1),
            h: ((self.height * self.scale).ceil() as u32).max(1),
        }
    }
}

pub type PreviewResult = (PreviewRequest, Result<ColorImage, String>);

pub struct Previewer {
    requests: Sender<PreviewRequest>,
    results: Receiver<PreviewResult>,
}

impl Previewer {
    pub fn spawn(wake: impl Fn() + Send + 'static) -> Self {
        let (req_tx, req_rx) = crossbeam_channel::unbounded::<PreviewRequest>();
        let (res_tx, res_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("vimpdf-preview".into())
            .spawn(move || {
                let mut renderer: Option<(PathBuf, PageRenderer)> = None;
                while let Ok(mut req) = req_rx.recv() {
                    // Only the latest request matters (the pointer moved on).
                    while let Ok(newer) = req_rx.try_recv() {
                        req = newer;
                    }
                    if renderer.as_ref().is_none_or(|(p, _)| *p != req.source) {
                        renderer = PageRenderer::open(&req.source)
                            .ok()
                            .map(|r| (req.source.clone(), r));
                    }
                    let image = match renderer.as_mut() {
                        Some((_, r)) => r
                            .render(req.page, req.scale, req.region())
                            .map(|mut p| {
                                if let Some(rc) = req.recolor {
                                    recolor(&mut p.rgb, rc);
                                }
                                ColorImage::from_rgb([p.width, p.height], &p.rgb)
                            })
                            .map_err(|e| format!("{e:#}")),
                        None => Err("cannot open the document".into()),
                    };
                    if res_tx.send((req, image)).is_err() {
                        return;
                    }
                    wake();
                }
            })
            .expect("failed to spawn preview thread");
        Self {
            requests: req_tx,
            results: res_rx,
        }
    }

    pub fn request(&self, req: PreviewRequest) {
        let _ = self.requests.send(req);
    }

    pub fn try_recv(&self) -> Option<PreviewResult> {
        self.results.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_requested_strip() {
        let path = std::env::temp_dir().join(format!("vimpdf-preview-{}.pdf", std::process::id()));
        std::fs::write(&path, crate::welcome::pdf_bytes()).unwrap();
        let previewer = Previewer::spawn(|| {});
        let req = PreviewRequest {
            source: path.clone(),
            page: 0,
            top: 40.0,
            width: 595.0,
            height: 150.0,
            scale: 1.5,
            recolor: None,
        };
        previewer.request(req.clone());
        let (got, image) = previewer
            .results
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert_eq!(got, req);
        let image = image.unwrap();
        assert_eq!(image.size, [893, 225]);
        // The welcome page's title is in that strip: not all white.
        assert!(image.pixels.iter().any(|p| p.r() < 128));
        std::fs::remove_file(&path).unwrap();
    }
}
