//! Everything tied to one open document: layout, render workers, the tile
//! texture cache and links. Replaced wholesale by `:open` and reload.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, TextureHandle, TextureOptions, Vec2,
};

use crate::document::DocInfo;
use crate::layout::Layout;
use crate::links::{self, PageLink};
use crate::render::cache::LruCache;
use crate::render::tiles;
use crate::render::{RenderJob, Renderer, ScaleKey};
use crate::search::{SearchState, Searcher};
use crate::viewport::Viewport;

const PLACEHOLDER_TEXT: Color32 = Color32::from_gray(160);
/// Space between pages, in PDF points.
const PAGE_GAP: f32 = 8.0;
/// After the zoom changes, wait this long for it to settle before rendering
/// at the new scale (the old tiles are drawn scaled meanwhile).
const RERENDER_DELAY: Duration = Duration::from_millis(150);
const CACHE_BUDGET: usize = 512 * 1024 * 1024;

pub struct DocView {
    pub path: PathBuf,
    pub layout: Layout,
    renderer: Renderer,
    textures: LruCache<RenderJob, TextureHandle>,
    failed: HashSet<RenderJob>,
    last_queue: Vec<RenderJob>,
    /// Render scale of the previous frame and when it last changed.
    scale: Option<(ScaleKey, Instant)>,
    pub links: Vec<Vec<PageLink>>,
    links_rx: Receiver<Vec<Vec<PageLink>>>,
    pub searcher: Searcher,
    pub search: SearchState,
}

impl DocView {
    /// Open `path`: reads page sizes here, starts render workers and the
    /// link loader in the background.
    pub fn open(ctx: &egui::Context, path: &Path) -> Result<Self> {
        let path = std::fs::canonicalize(path)
            .with_context(|| format!("cannot open '{}'", path.display()))?;
        let info = DocInfo::load(&path)?;
        Ok(Self::with_info(ctx, path, info))
    }

    /// Like `open`, with page sizes already loaded (and `path` canonical).
    pub fn with_info(ctx: &egui::Context, path: PathBuf, info: DocInfo) -> Self {
        tracing::info!(path = %path.display(), pages = info.page_count(), "opened document");

        let workers = thread_count();
        let wake = ctx.clone();
        let renderer = Renderer::spawn(path.clone(), workers, move || wake.request_repaint());
        let wake = ctx.clone();
        let links_rx = links::spawn_loader(path.clone(), move || wake.request_repaint());
        let wake = ctx.clone();
        let searcher = Searcher::spawn(path.clone(), move || wake.request_repaint());

        Self {
            layout: Layout::new(&info.page_sizes, PAGE_GAP),
            links: vec![Vec::new(); info.page_count()],
            path,
            renderer,
            textures: LruCache::new(CACHE_BUDGET),
            failed: HashSet::new(),
            last_queue: Vec::new(),
            scale: None,
            links_rx,
            searcher,
            search: SearchState::default(),
        }
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn page_count(&self) -> usize {
        self.layout.len()
    }

    pub fn receive_results(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.renderer.try_recv() {
            let job = result.job;
            match result.image {
                Ok(image) => {
                    let bytes = image.pixels.len() * 4;
                    let name = format!("p{}-{}-{}", job.page, job.tile.col, job.tile.row);
                    let tex = ctx.load_texture(name, image, TextureOptions::LINEAR);
                    self.textures.insert(job, tex, bytes);
                    tracing::trace!(
                        textures = self.textures.len(),
                        mb = self.textures.used_bytes() / (1024 * 1024),
                        "texture cache"
                    );
                }
                Err(err) => {
                    tracing::error!(page = job.page, "render failed: {err}");
                    self.failed.insert(job);
                }
            }
        }
        if let Ok(links) = self.links_rx.try_recv() {
            self.links = links;
        }
    }

    /// Queue tiles to render, unless the zoom is still changing.
    pub fn schedule(
        &mut self,
        ctx: &egui::Context,
        vp: &Viewport,
        scale: ScaleKey,
        scrolling_up: bool,
    ) {
        if self.scale.is_none_or(|(prev, _)| prev != scale) {
            self.scale = Some((scale, Instant::now()));
        }
        let since = self.scale.map_or(Duration::MAX, |(_, t)| t.elapsed());
        let first_render = self.textures.len() == 0;

        let jobs: Vec<RenderJob> = if since < RERENDER_DELAY && !first_render {
            ctx.request_repaint_after(RERENDER_DELAY - since);
            Vec::new()
        } else {
            tiles::plan(&self.layout, vp, scale, scrolling_up)
                .into_iter()
                .filter(|j| !self.textures.contains(j) && !self.failed.contains(j))
                .collect()
        };
        if jobs != self.last_queue {
            self.renderer.set_queue(jobs.iter().copied());
            self.last_queue = jobs;
        }
    }

    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        area: Rect,
        vp: &Viewport,
        scale: ScaleKey,
        page_color: Color32,
    ) {
        let ppp = painter.ctx().pixels_per_point();
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let visible = tiles::visible_tiles(&self.layout, vp, scale);

        for page in self.layout.pages_in(vp.y, vp.y + vp.doc_height()) {
            let r = self.layout.page(page);
            let (vx, vy) = vp.to_view(r.x, r.y);
            // Tiles are positioned from the page corner, which sits on the
            // pixel grid so that 1:1 tiles are not resampled and don't seam.
            let min = snap_to_pixels(area.min + Vec2::new(vx, vy), ppp);
            let page_rect = Rect::from_min_size(min, Vec2::new(r.w, r.h) * vp.zoom);
            painter.rect_filled(page_rect, 0.0, page_color);

            let wanted: Vec<&RenderJob> = visible.iter().filter(|j| j.page == page).collect();
            let complete = wanted.iter().all(|j| self.textures.contains(j));

            // Until all sharp tiles are in, draw the nearest other scale
            // stretched underneath.
            let mut drew_any = false;
            if !complete && let Some(other) = self.fallback_scale(page, scale) {
                let ratio = vp.zoom / other.scale();
                let keys: Vec<RenderJob> = self
                    .textures
                    .keys()
                    .filter(|k| k.page == page && k.scale == other)
                    .copied()
                    .collect();
                for key in keys {
                    let Some(tex) = self.textures.get(&key) else {
                        continue;
                    };
                    let rect = Rect::from_min_size(
                        min + Vec2::new(key.rect.x as f32, key.rect.y as f32) * ratio,
                        Vec2::new(key.rect.w as f32, key.rect.h as f32) * ratio,
                    );
                    if rect.intersects(area) {
                        painter.image(tex.id(), rect, uv, Color32::WHITE);
                        drew_any = true;
                    }
                }
            }

            for job in wanted {
                let Some(tex) = self.textures.get(job) else {
                    continue;
                };
                let rect = Rect::from_min_size(
                    min + Vec2::new(job.rect.x as f32, job.rect.y as f32) / ppp,
                    Vec2::new(job.rect.w as f32, job.rect.h as f32) / ppp,
                );
                painter.image(tex.id(), rect, uv, Color32::WHITE);
                drew_any = true;
            }

            if !drew_any {
                tracing::trace!(page, "drawing placeholder");
                painter.text(
                    page_rect.center(),
                    Align2::CENTER_CENTER,
                    (page + 1).to_string(),
                    FontId::proportional(24.0),
                    PLACEHOLDER_TEXT,
                );
            }
        }
    }

    /// The cached scale of `page` closest to `scale`, other than `scale`.
    fn fallback_scale(&self, page: usize, scale: ScaleKey) -> Option<ScaleKey> {
        self.textures
            .keys()
            .filter(|k| k.page == page && k.scale != scale)
            .map(|k| k.scale)
            .min_by_key(|s| s.distance(scale))
    }
}

fn thread_count() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    (cores / 2).clamp(2, 4)
}

/// Round a position to the nearest physical pixel.
fn snap_to_pixels(pos: Pos2, ppp: f32) -> Pos2 {
    Pos2::new((pos.x * ppp).round() / ppp, (pos.y * ppp).round() / ppp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_to_pixels_aligns_on_retina() {
        assert_eq!(
            snap_to_pixels(Pos2::new(10.3, 5.8), 2.0),
            Pos2::new(10.5, 6.0)
        );
    }
}
