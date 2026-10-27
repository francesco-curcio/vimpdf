use std::collections::HashSet;
use std::ops::Range;
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
/// Pages rendered ahead in the scroll direction, and behind it.
const PREFETCH_AHEAD: usize = 4;
const PREFETCH_BEHIND: usize = 1;
const CACHE_BUDGET: usize = 512 * 1024 * 1024;
const MESSAGE_DURATION: Duration = Duration::from_secs(4);

pub struct PdfvApp {
    layout: Layout,
    viewport: Option<Viewport>,
    renderer: Renderer,
    textures: LruCache<RenderJob, TextureHandle>,
    failed: HashSet<RenderJob>,
    last_queue: Vec<RenderJob>,
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
        tracing::info!("{msg}");
        self.message = Some((msg, Instant::now()));
    }

    fn execute(&mut self, cmd: Command, ctx: &egui::Context) {
        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        let layout = &self.layout;
        let screen = vp.height;
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
            match result.image {
                Ok(image) => {
                    let bytes = image.pixels.len() * 4;
                    let tex = ctx.load_texture(
                        format!("page-{}", result.job.page),
                        image,
                        TextureOptions::LINEAR,
                    );
                    self.textures.insert(result.job, tex, bytes);
                    tracing::debug!(
                        textures = self.textures.len(),
                        mb = self.textures.used_bytes() / (1024 * 1024),
                        "texture cache"
                    );
                }
                Err(err) => {
                    tracing::error!(page = result.job.page, "render failed: {err}");
                    self.failed.insert(result.job);
                }
            }
        }
        if let Ok(links) = self.links_rx.try_recv() {
            self.links = links;
        }
    }

    fn schedule(&mut self, vp: &Viewport, scale: ScaleKey) {
        let visible = self.layout.pages_in(vp.y, vp.y + vp.doc_height());
        let jobs: Vec<RenderJob> = render_order(visible, self.layout.len(), self.scrolling_up)
            .into_iter()
            .map(|page| RenderJob { page, scale })
            .filter(|j| !self.textures.contains(j) && !self.failed.contains(j))
            .collect();
        if jobs != self.last_queue {
            self.renderer.set_queue(jobs.iter().copied());
            self.last_queue = jobs;
        }
    }

    /// The cached render of `page` closest to `scale`, for drawing while the
    /// sharp one is pending.
    fn fallback_texture(&mut self, page: usize, scale: ScaleKey) -> Option<egui::TextureId> {
        let key = self
            .textures
            .keys()
            .filter(|k| k.page == page)
            .min_by_key(|k| k.scale.distance(scale))
            .copied()?;
        self.textures.get(&key).map(|t| t.id())
    }

    fn draw_pages(&mut self, painter: &egui::Painter, area: Rect, vp: &Viewport, scale: ScaleKey) {
        let ppp = painter.ctx().pixels_per_point();
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        for page in self.layout.pages_in(vp.y, vp.y + vp.doc_height()) {
            let r = self.layout.page(page);
            let (vx, vy) = vp.to_view(r.x, r.y);
            let min = area.min + Vec2::new(vx, vy);
            let page_rect = Rect::from_min_size(min, Vec2::new(r.w, r.h) * vp.zoom);

            let exact = RenderJob { page, scale };
            if let Some(tex) = self.textures.get(&exact) {
                // Draw 1:1 on the physical pixel grid so it is not resampled.
                let rect = snap_to_pixels(Rect::from_min_size(min, tex.size_vec2() / ppp), ppp);
                painter.image(tex.id(), rect, uv, Color32::WHITE);
            } else if let Some(tex) = self.fallback_texture(page, scale) {
                painter.image(tex, page_rect, uv, Color32::WHITE);
            } else {
                tracing::trace!(page, "drawing placeholder");
                painter.rect_filled(page_rect, 0.0, Color32::WHITE);
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
}

/// Visible pages first, then pages ahead in the scroll direction, then behind.
fn render_order(visible: Range<usize>, page_count: usize, up: bool) -> Vec<usize> {
    let below = (visible.end..page_count).take(if up { PREFETCH_BEHIND } else { PREFETCH_AHEAD });
    let above = (0..visible.start)
        .rev()
        .take(if up { PREFETCH_AHEAD } else { PREFETCH_BEHIND });
    let mut pages: Vec<usize> = visible.collect();
    if up {
        pages.extend(above);
        pages.extend(below);
    } else {
        pages.extend(below);
        pages.extend(above);
    }
    pages
}

fn thread_count() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    (cores / 2).clamp(2, 4)
}

/// Move a rect so its top-left corner sits on a physical pixel, keeping its size.
fn snap_to_pixels(rect: Rect, ppp: f32) -> Rect {
    let min = Pos2::new(
        (rect.min.x * ppp).round() / ppp,
        (rect.min.y * ppp).round() / ppp,
    );
    Rect::from_min_size(min, rect.size())
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
            vp.fit_width(layout, PAGE_MARGIN);
        }

        // Input → commands.
        let events = ctx.input(|i| i.events.clone());
        let mut commands: Vec<Command> = key_tokens(&events)
            .iter()
            .filter_map(|tok| self.keys.feed(tok, &self.keymap))
            .collect();
        let mouse = mouse::handle(&ctx, area, vp, &self.layout, &self.links);
        commands.extend(mouse.commands);
        let y_before = vp.y;
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

        self.receive_results(&ctx);
        let scale = ScaleKey::from_scale(vp.zoom * ctx.pixels_per_point());
        self.schedule(&vp, scale);

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
    fn render_order_prefers_scroll_direction() {
        assert_eq!(render_order(3..5, 20, false), vec![3, 4, 5, 6, 7, 8, 2]);
        assert_eq!(render_order(3..5, 20, true), vec![3, 4, 2, 1, 0, 5]);
        assert_eq!(render_order(18..20, 20, false), vec![18, 19, 17]);
        // Viewport inside a gap: nothing visible, still prefetch around it.
        assert_eq!(render_order(5..5, 20, false), vec![5, 6, 7, 8, 4]);
    }

    #[test]
    fn snap_to_pixels_aligns_on_retina() {
        let r = snap_to_pixels(
            Rect::from_min_size(Pos2::new(10.3, 5.8), Vec2::splat(4.0)),
            2.0,
        );
        assert_eq!(r.min, Pos2::new(10.5, 6.0));
        assert_eq!(r.size(), Vec2::splat(4.0));
    }
}
