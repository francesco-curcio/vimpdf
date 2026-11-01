//! Link preview: resting the pointer on an internal link (a citation, a
//! reference to a figure or an equation) shows where it leads, in a popup,
//! without jumping there.

use super::*;
use crate::render::preview::{PreviewRequest, Previewer};

/// How long the pointer rests on a link before the preview shows.
const DELAY: Duration = Duration::from_millis(350);
/// Size of the preview image, in screen points, at most: smaller windows
/// get a preview up to 70% of their width and 45% of their height.
const WIDTH: f32 = 820.0;
const HEIGHT: f32 = 320.0;
/// Page points shown above the destination (like a jump's context).
const CONTEXT: f32 = 14.0;

#[derive(Default)]
pub(super) struct LinkPreview {
    /// The link under the pointer (page and position it leads to) and since
    /// when.
    hovered: Option<((usize, Option<f32>), Instant)>,
    requested: Option<PreviewRequest>,
    shown: Option<(PreviewRequest, egui::TextureHandle)>,
    previewer: Option<Previewer>,
}

/// The strip of `page` (`width` × `height` points) to show for a link to
/// `y`: starting a little above it, kept inside the page.
fn strip_top(y: Option<f32>, page_height: f32, height: f32) -> f32 {
    let top = y.map_or(0.0, |y| y - CONTEXT);
    top.min(page_height - height).max(0.0)
}

impl VimpdfApp {
    /// Follow the pointer; once it has rested on an internal link, show the
    /// preview next to it.
    pub(super) fn show_link_preview(&mut self, ctx: &egui::Context, area: Rect, vp: &Viewport) {
        let target = self.hovered_internal_link(ctx, area, vp);
        let preview = &mut self.link_preview;
        let Some((target, pointer)) = target else {
            preview.hovered = None;
            return;
        };
        let since = match preview.hovered {
            Some((t, since)) if t == target => since,
            _ => {
                preview.hovered = Some((target, Instant::now()));
                Instant::now()
            }
        };
        let waited = since.elapsed();
        if waited < DELAY {
            ctx.request_repaint_after(DELAY - waited);
            return;
        }

        let (page, y) = target;
        let r = self.view.layout.page(page);
        let screen = ctx.content_rect();
        let zoom = WIDTH.min(screen.width() * 0.7) / r.w;
        let height = (HEIGHT.min(screen.height() * 0.45) / zoom).min(r.h);
        let request = PreviewRequest {
            source: self.view.render_source.clone(),
            page,
            top: strip_top(y, r.h, height),
            width: r.w,
            height,
            scale: zoom * ctx.pixels_per_point(),
            recolor: self.recolor(),
        };
        let preview = &mut self.link_preview;
        let previewer = preview.previewer.get_or_insert_with(|| {
            let wake = ctx.clone();
            Previewer::spawn(move || wake.request_repaint())
        });
        if preview.requested.as_ref() != Some(&request) {
            previewer.request(request.clone());
            preview.requested = Some(request.clone());
        }
        while let Some((req, image)) = previewer.try_recv() {
            match image {
                Ok(image) if preview.requested.as_ref() == Some(&req) => {
                    let texture = ctx.load_texture(
                        "vimpdf-link-preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    preview.shown = Some((req, texture));
                }
                Ok(_) => {}
                Err(e) => tracing::debug!("link preview failed: {e}"),
            }
        }
        let Some((_, texture)) = preview.shown.as_ref().filter(|(req, _)| *req == request) else {
            return;
        };

        // Below and right of the pointer; above it if there's no room below,
        // and kept inside the window.
        let size = Vec2::new(r.w * zoom, height * zoom);
        let outer = size + Vec2::new(16.0, 16.0 + self.config.font_size * 1.4);
        let mut pos = pointer + Vec2::new(12.0, 18.0);
        if pos.y + outer.y > screen.max.y {
            pos.y = pointer.y - outer.y - 8.0;
        }
        pos.x = pos.x.min(screen.max.x - outer.x).max(screen.min.x);
        pos.y = pos.y.max(screen.min.y);
        let id = texture.id();
        egui::Area::new(egui::Id::unique("vimpdf-link-preview"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.image((id, size));
                    ui.label(
                        egui::RichText::new(format!("Page {}", page + 1))
                            .color(ui::BAR_DIM)
                            .size(self.config.font_size * 0.85),
                    );
                });
            });
    }

    /// The internal link under the pointer, as (page, y) it leads to, and
    /// the pointer, when a preview may show: the option is on, nothing else
    /// has the pointer, and no button is held.
    fn hovered_internal_link(
        &self,
        ctx: &egui::Context,
        area: Rect,
        vp: &Viewport,
    ) -> Option<((usize, Option<f32>), egui::Pos2)> {
        let calm = self.config.link_preview
            && matches!(self.mode, Mode::Normal | Mode::Visual)
            && self.menu.is_none()
            && self.settings.is_none()
            && self.start.is_none()
            && !ctx.egui_wants_pointer_input();
        if !calm || ctx.input(|i| i.pointer.any_down()) {
            return None;
        }
        let pointer = ctx.input(|i| i.pointer.hover_pos())?;
        if !area.contains(pointer) {
            return None;
        }
        let link = mouse::link_at(
            pointer - area.min.to_vec2(),
            vp,
            &self.view.layout,
            &self.view.links,
        )?;
        match link.target {
            LinkTarget::Internal { page, y, .. } if page < self.view.layout.len() => {
                Some(((page, y), pointer))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_starts_above_the_destination_and_stays_on_the_page() {
        assert_eq!(strip_top(Some(300.0), 800.0, 200.0), 300.0 - CONTEXT);
        assert_eq!(strip_top(Some(5.0), 800.0, 200.0), 0.0, "top of the page");
        assert_eq!(strip_top(Some(790.0), 800.0, 200.0), 600.0, "bottom");
        assert_eq!(strip_top(None, 800.0, 200.0), 0.0, "no position: the top");
    }
}
