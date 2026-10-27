use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, TextureHandle, TextureOptions, Vec2,
};

use crate::document::DocInfo;
use crate::input::command::Command;
use crate::input::keymap::{KeyState, Keymap};
use crate::input::{key_tokens, mouse};
use crate::layout::Layout;
use crate::links::{self, LinkTarget, PageLink};
use crate::render::cache::LruCache;
use crate::render::tiles;
use crate::render::{RenderJob, Renderer, ScaleKey};
use crate::viewport::Viewport;

const BACKGROUND: Color32 = Color32::from_gray(40);
const PLACEHOLDER_TEXT: Color32 = Color32::from_gray(160);
/// Space left and right of pages in fit-width mode, in screen points.
const PAGE_MARGIN: f32 = 16.0;
/// Space between pages, in PDF points.
const PAGE_GAP: f32 = 8.0;
/// Distance of one `j`/`k` step, in screen points.
const SCROLL_STEP: f32 = 60.0;
/// Zoom factor of one `+`/`-` step.
const ZOOM_STEP: f32 = 1.1;
/// After the zoom changes, wait this long for it to settle before rendering
/// at the new scale (the old tiles are drawn scaled meanwhile).
const RERENDER_DELAY: Duration = Duration::from_millis(150);
const CACHE_BUDGET: usize = 512 * 1024 * 1024;
const MESSAGE_DURATION: Duration = Duration::from_secs(4);

pub struct PdfvApp {
    layout: Layout,
    viewport: Option<Viewport>,
    renderer: Renderer,
    textures: LruCache<RenderJob, TextureHandle>,
    failed: HashSet<RenderJob>,
    last_queue: Vec<RenderJob>,
    /// Render scale of the previous frame and when it last changed.
    scale: Option<(ScaleKey, Instant)>,
    /// Direction of the last vertical movement, for prefetching.
    scrolling_up: bool,
    keymap: Keymap,
    keys: KeyState,
    links: Vec<Vec<PageLink>>,
    links_rx: Receiver<Vec<Vec<PageLink>>>,
    message: Option<(String, Instant)>,
}

impl PdfvApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: PathBuf, info: DocInfo) -> Self {
        // Cmd +/- would change egui's UI scale; zoom is ours to handle.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);

        let workers = thread_count();
        tracing::debug!(workers, "starting render workers");
        let ctx = cc.egui_ctx.clone();
        let renderer = Renderer::spawn(path.clone(), workers, move || ctx.request_repaint());
        let ctx = cc.egui_ctx.clone();
        let links_rx = links::spawn_loader(path, move || ctx.request_repaint());

        Self {
            layout: Layout::new(&info.page_sizes, PAGE_GAP),
            viewport: None,
            renderer,
            textures: LruCache::new(CACHE_BUDGET),
            failed: HashSet::new(),
            last_queue: Vec::new(),
            scale: None,
            scrolling_up: false,
            keymap: Keymap::default_normal(),
            keys: KeyState::default(),
            links: vec![Vec::new(); info.page_count()],
            links_rx,
            message: None,
        }
    }

    fn show_message(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        tracing::debug!("{msg}");
        self.message = Some((msg, Instant::now()));
    }

    fn execute(&mut self, cmd: Command, ctx: &egui::Context) {
        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        let layout = &self.layout;
        let screen = vp.height;
        let (cx, cy) = (vp.width / 2.0, vp.height / 2.0);
        match cmd {
            Command::ScrollDown(n) => vp.scroll_by(0.0, n as f32 * SCROLL_STEP),
            Command::ScrollUp(n) => vp.scroll_by(0.0, -(n as f32) * SCROLL_STEP),
            Command::ScrollLeft(n) => vp.scroll_by(-(n as f32) * SCROLL_STEP, 0.0),
            Command::ScrollRight(n) => vp.scroll_by(n as f32 * SCROLL_STEP, 0.0),
            Command::ScrollBy { dx, dy } => vp.scroll_by(dx, dy),
            Command::HalfPageDown(n) => vp.scroll_by(0.0, n as f32 * screen / 2.0),
            Command::HalfPageUp(n) => vp.scroll_by(0.0, -(n as f32) * screen / 2.0),
            Command::ScreenDown(n) => vp.scroll_by(0.0, n as f32 * screen),
            Command::ScreenUp(n) => vp.scroll_by(0.0, -(n as f32) * screen),
            Command::NextPage(n) => vp.next_page(layout, n as usize),
            Command::PrevPage(n) => vp.prev_page(layout, n as usize),
            Command::GotoFirst => vp.y = 0.0,
            Command::GotoLast => vp.goto_end(layout),
            Command::GotoPage(n) => vp.goto_page(layout, n.saturating_sub(1)),
            Command::ZoomIn(n) => vp.zoom_by(ZOOM_STEP.powi(n as i32), cx, cy),
            Command::ZoomOut(n) => vp.zoom_by(ZOOM_STEP.powi(-(n as i32)), cx, cy),
            Command::ZoomReset => vp.zoom_to(1.0, cx, cy),
            Command::ZoomBy { factor, x, y } => vp.zoom_by(factor, x, y),
            Command::FitWidth => vp.fit_width(layout, PAGE_MARGIN),
            Command::FitPage => vp.fit_page(layout, PAGE_MARGIN),
            Command::FollowLink(LinkTarget::Internal { page, .. }) => {
                // Exact destination positions come with the jumplist (M5).
                vp.goto_page(layout, page);
            }
            Command::FollowLink(LinkTarget::External(uri)) => match links::open_external(&uri) {
                Ok(()) => self.show_message(format!("Opening {uri}")),
                Err(err) => self.show_message(err),
            },
            Command::Cancel => self.message = None,
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn receive_results(&mut self, ctx: &egui::Context) {
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
    fn schedule(&mut self, ctx: &egui::Context, vp: &Viewport, scale: ScaleKey) {
        let changed = match self.scale {
            Some((prev, _)) if prev == scale => false,
            _ => {
                self.scale = Some((scale, Instant::now()));
                true
            }
        };
        let since = self.scale.map_or(Duration::MAX, |(_, t)| t.elapsed());
        let first_render = self.textures.len() == 0;

        let jobs: Vec<RenderJob> = if since < RERENDER_DELAY && !first_render {
            if changed {
                tracing::trace!(scale = scale.scale(), "waiting for zoom to settle");
            }
            ctx.request_repaint_after(RERENDER_DELAY - since);
            Vec::new()
        } else {
            tiles::plan(&self.layout, vp, scale, self.scrolling_up)
                .into_iter()
                .filter(|j| !self.textures.contains(j) && !self.failed.contains(j))
                .collect()
        };
        if jobs != self.last_queue {
            self.renderer.set_queue(jobs.iter().copied());
            self.last_queue = jobs;
        }
    }

    fn draw_pages(&mut self, painter: &egui::Painter, area: Rect, vp: &Viewport, scale: ScaleKey) {
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
            painter.rect_filled(page_rect, 0.0, Color32::WHITE);

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

impl eframe::App for PdfvApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let area = ui.max_rect();

        let layout = &self.layout;
        let vp = self.viewport.get_or_insert_with(|| {
            let mut vp = Viewport::new(area.width(), area.height());
            vp.fit_width(layout, PAGE_MARGIN);
            vp
        });
        if (vp.width, vp.height) != (area.width(), area.height()) {
            vp.resize(area.width(), area.height());
            vp.refit(layout, PAGE_MARGIN);
        }

        // Input → commands.
        let events = ctx.input(|i| i.events.clone());
        let mut commands: Vec<Command> = key_tokens(&events)
            .iter()
            .filter_map(|tok| self.keys.feed(tok, &self.keymap))
            .collect();
        let mouse = mouse::handle(&ctx, area, vp, &self.layout, &self.links);
        commands.extend(mouse.commands);
        let (y_before, zoom_before) = (vp.y, vp.zoom);
        for cmd in commands {
            tracing::trace!(?cmd, "command");
            self.execute(cmd, &ctx);
        }

        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        vp.clamp(&self.layout);
        if vp.y != y_before {
            self.scrolling_up = vp.y < y_before;
            tracing::trace!(page = vp.current_page(&self.layout) + 1, y = vp.y, "moved");
        }
        let vp = vp.clone();
        if vp.zoom != zoom_before {
            // Until the status bar exists (M3).
            self.show_message(format!("Zoom {:.0}%", vp.zoom * 100.0));
        }

        self.receive_results(&ctx);
        let scale = ScaleKey::from_scale(vp.zoom * ctx.pixels_per_point());
        self.schedule(&ctx, &vp, scale);

        let painter = ui.painter().clone();
        painter.rect_filled(area, 0.0, BACKGROUND);
        self.draw_pages(&painter, area, &vp, scale);

        if let Some((msg, since)) = &self.message {
            if since.elapsed() < MESSAGE_DURATION {
                painter.text(
                    area.left_bottom() + Vec2::new(8.0, -8.0),
                    Align2::LEFT_BOTTOM,
                    msg,
                    FontId::proportional(14.0),
                    Color32::LIGHT_GRAY,
                );
                ctx.request_repaint_after(MESSAGE_DURATION - since.elapsed());
            } else {
                self.message = None;
            }
        }

        ctx.set_cursor_icon(mouse.cursor);
    }
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
