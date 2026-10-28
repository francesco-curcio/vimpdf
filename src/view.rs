//! Everything tied to one open document: layout, render workers, the tile
//! texture cache and links. Replaced wholesale by `:open` and reload.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, TextureHandle, TextureOptions, Vec2,
};

use crate::annotate::Editor;
use crate::document::{DocInfo, OutlineItem};
use crate::layout::Layout;
use crate::links::{self, PageLink};
use crate::render::cache::LruCache;
use crate::render::tiles;
use crate::render::{Recolor, RenderJob, Renderer, ScaleKey};
use crate::search::{SearchState, Searcher};
use crate::selection::DocText;
use crate::text::PageText;
use crate::viewport::Viewport;

const PLACEHOLDER_TEXT: Color32 = Color32::from_gray(160);
/// Space between pages, in PDF points.
const PAGE_GAP: f32 = 8.0;
/// After the zoom changes, wait this long for it to settle before rendering
/// at the new scale (the old tiles are drawn scaled meanwhile).
const RERENDER_DELAY: Duration = Duration::from_millis(150);
/// The rendered tiles kept (in GPU memory) are what the view and its
/// prefetch need, with room for the previous zoom's tiles while new ones
/// render and for scrolling back a little: 1.5 times the plan, within these
/// bounds.
const MIN_CACHE: usize = 48 * 1024 * 1024;
const MAX_CACHE: usize = 256 * 1024 * 1024;

fn cache_budget(plan: &[RenderJob]) -> usize {
    let needed: usize = plan
        .iter()
        .map(|j| (j.rect.w * j.rect.h) as usize * 4)
        .sum();
    (needed * 3 / 2).clamp(MIN_CACHE, MAX_CACHE)
}

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
    pub outline: Vec<OutlineItem>,
    pub metadata: Vec<(&'static str, String)>,
    pub fingerprint: Option<String>,
    links_rx: Receiver<links::Navigation>,
    pub searcher: Searcher,
    /// Annotation edits (highlights, comments, redaction marks).
    pub editor: Editor,
    pub search: SearchState,
    /// Page text loaded so far (for selection and hit-testing).
    texts: HashMap<usize, Arc<PageText>>,
    texts_requested: HashSet<usize>,
    /// Per page, bumped when an annotation edit changes it.
    generations: Vec<u32>,
}

impl DocView {
    /// Open `path`: reads page sizes here, starts render workers and the
    /// link loader in the background.
    pub fn open(ctx: &egui::Context, path: &Path, author: String) -> Result<Self> {
        let path = std::fs::canonicalize(path)
            .with_context(|| format!("cannot open '{}'", path.display()))?;
        let info = DocInfo::load(&path)?;
        Ok(Self::with_info(ctx, path, info, author))
    }

    /// Like `open`, with page sizes already loaded (and `path` canonical).
    pub fn with_info(ctx: &egui::Context, path: PathBuf, info: DocInfo, author: String) -> Self {
        tracing::info!(path = %path.display(), pages = info.page_count(), "opened document");

        let workers = thread_count();
        let wake = ctx.clone();
        let renderer = Renderer::spawn(path.clone(), workers, move || wake.request_repaint());
        let wake = ctx.clone();
        let links_rx = links::spawn_loader(path.clone(), move || wake.request_repaint());
        let wake = ctx.clone();
        let searcher = Searcher::spawn(path.clone(), move || wake.request_repaint());
        let wake = ctx.clone();
        let editor = Editor::spawn(path.clone(), author, move || wake.request_repaint());

        Self {
            layout: Layout::new(&info.page_sizes, PAGE_GAP),
            links: vec![Vec::new(); info.page_count()],
            outline: Vec::new(),
            metadata: info.metadata.clone(),
            fingerprint: info.fingerprint.clone(),
            path,
            renderer,
            textures: LruCache::new(MIN_CACHE),
            failed: HashSet::new(),
            last_queue: Vec::new(),
            scale: None,
            links_rx,
            searcher,
            editor,
            search: SearchState::default(),
            texts: HashMap::new(),
            texts_requested: HashSet::new(),
            generations: vec![0; info.page_count()],
        }
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Render from `path` (a working copy with edits) and re-render `pages`.
    /// Their old tiles stay on screen until the new ones arrive.
    pub fn content_changed(&mut self, path: PathBuf, pages: &[usize]) {
        self.renderer.set_source(path);
        for &p in pages {
            if let Some(g) = self.generations.get_mut(p) {
                *g += 1;
            }
        }
        self.last_queue.clear();
    }

    pub fn add_text(&mut self, page: usize, text: Arc<PageText>) {
        self.texts.insert(page, text);
    }

    pub fn text(&self, page: usize) -> Option<&PageText> {
        self.texts.get(&page).map(|t| t.as_ref())
    }

    pub fn doc_text(&self) -> DocText<'_> {
        DocText {
            pages: &self.texts,
            page_count: self.page_count(),
        }
    }

    /// Ask for the text of the visible pages and their neighbours.
    pub fn request_text(&mut self, vp: &Viewport) {
        let visible = self.layout.pages_in(vp.y, vp.y + vp.doc_height());
        let from = visible.start.saturating_sub(1);
        let to = (visible.end + 1).min(self.page_count());
        for page in from..to {
            if self.texts_requested.insert(page) {
                self.searcher.request_text(page);
            }
        }
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
        if let Ok((links, outline)) = self.links_rx.try_recv() {
            self.links = links;
            self.outline = outline;
        }
    }

    /// Queue tiles to render, unless the zoom is still changing.
    pub fn schedule(
        &mut self,
        ctx: &egui::Context,
        vp: &Viewport,
        scale: ScaleKey,
        recolor: Option<Recolor>,
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
            let plan = tiles::plan(
                &self.layout,
                &self.generations,
                vp,
                scale,
                recolor,
                scrolling_up,
            );
            self.textures.set_budget(cache_budget(&plan));
            plan.into_iter()
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
        recolor: Option<Recolor>,
        page_color: Color32,
    ) {
        let ppp = painter.ctx().pixels_per_point();
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let visible = tiles::visible_tiles(&self.layout, &self.generations, vp, scale, recolor);

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
            let generation = self.generations[page];
            if !complete
                && let Some((other, other_rc, other_gen)) =
                    self.fallback(page, generation, scale, recolor)
            {
                let ratio = vp.zoom / other.scale();
                let keys: Vec<RenderJob> = self
                    .textures
                    .keys()
                    .filter(|k| {
                        k.page == page
                            && k.scale == other
                            && k.recolor == other_rc
                            && k.generation == other_gen
                    })
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

    /// The cached render of `page` to draw stretched while the sharp tiles
    /// are missing: same colours if possible, then the closest scale.
    fn fallback(
        &self,
        page: usize,
        generation: u32,
        scale: ScaleKey,
        recolor: Option<Recolor>,
    ) -> Option<(ScaleKey, Option<Recolor>, u32)> {
        self.textures
            .keys()
            .filter(|k| {
                k.page == page && (k.scale, k.recolor, k.generation) != (scale, recolor, generation)
            })
            .map(|k| (k.scale, k.recolor, k.generation))
            .min_by_key(|(s, rc, g)| (*rc != recolor, s.distance(scale), *g != generation))
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

    /// Scroll a 36-page paper at fit width in a 2690×1570 px window, top to
    /// bottom and back, the way `schedule` and `receive_results` fill the
    /// cache. Returns the peak bytes held.
    fn simulate_reading() -> usize {
        let sizes = vec![
            crate::document::PageSize {
                width: 612.0,
                height: 792.0,
                origin_x: 0.0,
                origin_y: 0.0,
            };
            36
        ];
        let layout = Layout::new(&sizes, PAGE_GAP);
        let generations = vec![0; 36];
        let mut vp = Viewport::new(2690.0, 1570.0);
        vp.fit_width(&layout, 16.0);
        let scale = ScaleKey::from_scale(vp.zoom);
        let mut cache: LruCache<RenderJob, ()> = LruCache::new(MIN_CACHE);
        let mut peak = 0;
        let end = layout.height() - vp.doc_height();
        let step = vp.doc_height() / 4.0;
        let mut ys: Vec<f32> = (0..)
            .map(|i| i as f32 * step)
            .take_while(|y| *y < end)
            .collect();
        let back: Vec<f32> = ys.iter().rev().copied().collect();
        let down = ys.len();
        ys.extend(back);
        for (i, y) in ys.into_iter().enumerate() {
            vp.y = y;
            let plan = tiles::plan(&layout, &generations, &vp, scale, None, i >= down);
            cache.set_budget(cache_budget(&plan));
            for job in plan {
                if !cache.contains(&job) {
                    cache.insert(job, (), (job.rect.w * job.rect.h * 4) as usize);
                }
            }
            peak = peak.max(cache.used_bytes());
        }
        peak
    }

    #[test]
    fn reading_a_paper_keeps_the_tile_cache_small() {
        let peak = simulate_reading();
        eprintln!("SIM peak tile cache: {} MiB", peak / (1024 * 1024));
        // It was 512 MiB (the whole old budget) before the cache followed
        // the plan.
        assert!(peak <= 128 * 1024 * 1024, "{} MiB", peak / (1024 * 1024));
    }

    #[test]
    fn snap_to_pixels_aligns_on_retina() {
        assert_eq!(
            snap_to_pixels(Pos2::new(10.3, 5.8), 2.0),
            Pos2::new(10.5, 6.0)
        );
    }
}
