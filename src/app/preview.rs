//! Link preview: resting the pointer on an internal link (a citation, a
//! reference to a figure or an equation) shows where it leads, in a popup,
//! without jumping there. `gp` opens the same popup from the keyboard.
//!
//! The popup is a small view of the document: it scrolls across pages and
//! its links work, so a reference's own link can be followed from it.

use std::collections::HashMap;
use std::ops::Range;

use super::*;
use crate::links::{HintAction, PageLink};
use crate::render::preview::{PreviewRequest, Previewer};

/// How long the pointer rests on a link before the preview shows.
const DELAY: Duration = Duration::from_millis(350);
/// How long the popup waits for a pointer that left the link to reach it.
const GRACE: Duration = Duration::from_millis(300);
/// Size of the preview image, in screen points, at most: smaller windows
/// get a preview up to 70% of their width and 45% of their height.
const WIDTH: f32 = 820.0;
const HEIGHT: f32 = 320.0;
/// Page points shown above the destination (like a jump's context).
const CONTEXT: f32 = 14.0;
/// Rendered pages kept around the ones in view, to scroll back to.
const KEEP_PAGES: usize = 2;

/// The open popup. `left` and `top` are the document point at its corner.
struct Popup {
    target: LinkTarget,
    left: f32,
    top: f32,
    zoom: f32,
    /// The pages' part of the popup, in screen points.
    size: Vec2,
    pos: egui::Pos2,
    /// On screen, once drawn.
    rect: Option<Rect>,
    /// Page and index of the link chosen with `<Tab>`.
    selected: Option<(usize, usize)>,
    /// When the pointer left the link and the popup.
    left_at: Option<Instant>,
}

impl Popup {
    fn doc_height(&self) -> f32 {
        self.size.y / self.zoom
    }

    fn scroll(&mut self, dy: f32, total: f32) {
        self.top = (self.top + dy).min(total - self.doc_height()).max(0.0);
    }
}

#[derive(Default)]
pub(super) struct LinkPreview {
    /// The link under the pointer and since when.
    hovered: Option<(LinkTarget, Instant)>,
    popup: Option<Popup>,
    /// Whole pages, rendered one at a time.
    pages: HashMap<usize, (PreviewRequest, egui::TextureHandle)>,
    requested: Option<PreviewRequest>,
    previewer: Option<Previewer>,
}

impl LinkPreview {
    pub(super) fn close(&mut self) {
        self.popup = None;
        self.pages.clear();
        self.requested = None;
    }
}

/// The strip of `page` (`width` × `height` points) to show for a link to
/// `y`: starting a little above it, kept inside the page.
fn strip_top(y: Option<f32>, page_height: f32, height: f32) -> f32 {
    let top = y.map_or(0.0, |y| y - CONTEXT);
    top.min(page_height - height).max(0.0)
}

/// The position after `current` in a list of `len`, wrapping around.
fn step(len: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (current, forward) {
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
        (None, true) => 0,
        (None, false) => len - 1,
    })
}

impl VimpdfApp {
    /// Follow the pointer, and draw the popup if it's open. Returns what a
    /// click in it asks for, and the cursor over it.
    pub(super) fn show_link_preview(
        &mut self,
        ctx: &egui::Context,
        area: Rect,
        vp: &Viewport,
    ) -> (Option<Command>, Option<egui::CursorIcon>) {
        let keyboard = self.mode == Mode::LinkPreview;
        if keyboard && self.link_preview.popup.is_none() {
            self.mode = Mode::Normal; // closed behind its back: a reload
        }
        let hover = self.hovered_internal_link(ctx, area, vp);
        if self.link_preview.popup.is_some() {
            if !keyboard && !self.popup_stays(ctx, hover.as_ref().map(|(t, _)| t)) {
                self.link_preview.close();
            }
        } else if let Some((target, pointer)) = self.rested_link(ctx, hover) {
            self.open_popup(ctx, target, Some(pointer));
        }

        let Some(mut popup) = self.link_preview.popup.take() else {
            return (None, None);
        };
        let shown = self.request_pages(ctx, &popup);
        if !shown && popup.rect.is_none() {
            self.link_preview.popup = Some(popup);
            return (None, None);
        }
        let (mut command, mut cursor) = (None, None);
        let response = egui::Area::new(egui::Id::unique("vimpdf-link-preview"))
            .order(egui::Order::Tooltip)
            .fixed_pos(popup.pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let (rect, response) = ui.allocate_exact_size(popup.size, egui::Sense::click());
                    if response.hovered() {
                        let dy = -ctx.input(|i| i.smooth_scroll_delta.y) / popup.zoom;
                        popup.scroll(dy, self.view.layout.height());
                    }
                    self.draw_popup_pages(ui, rect, &popup);
                    let link = response
                        .hover_pos()
                        .and_then(|p| self.link_in_popup(&popup, rect, p));
                    if let Some(link) = link {
                        cursor = Some(egui::CursorIcon::PointingHand);
                        if response.clicked() {
                            command = Some(Command::FollowLink(link.target.clone()));
                        }
                    }
                    let page = self.view.layout.pages_in(popup.top, popup.top).start;
                    ui.label(
                        egui::RichText::new(format!("Page {}", page + 1))
                            .color(ui::BAR_DIM)
                            .size(self.config.font_size * 0.85),
                    );
                });
            });
        popup.rect = Some(response.response.rect);
        let clicked_away = ctx.input(|i| {
            i.pointer.primary_clicked()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !response.response.rect.contains(p))
        });
        self.link_preview.popup = Some(popup);
        if command.is_some() || (keyboard && clicked_away) {
            self.close_link_preview();
        }
        (command, cursor)
    }

    /// `gp`: show where a link leads, with the keyboard on the popup.
    pub(super) fn preview_link(&mut self, ctx: &egui::Context, target: LinkTarget) {
        self.link_preview.close();
        self.open_popup(ctx, target, None);
        if self.link_preview.popup.is_some() {
            self.mode = Mode::LinkPreview;
            self.info("<Tab> picks a link, Enter follows it, Esc closes");
        }
    }

    pub(super) fn close_link_preview(&mut self) {
        self.link_preview.close();
        if self.mode == Mode::LinkPreview {
            self.mode = Mode::Normal;
        }
    }

    /// A key while the popup has the keyboard: the scrolling keys scroll it.
    pub(super) fn preview_key(&mut self, tok: &str) -> Option<Command> {
        if !self.keys.is_pending() {
            match tok {
                "<Tab>" => return self.preview_select(true),
                "<S-Tab>" => return self.preview_select(false),
                "<CR>" => return self.preview_follow(),
                _ => {}
            }
        }
        let cmd = self.keys.feed(tok, &self.keymap)?;
        let total = self.view.layout.height();
        let step = self.config.scroll_step;
        let popup = self.link_preview.popup.as_mut()?;
        let line = step / popup.zoom;
        let screen = popup.doc_height();
        let dy = match cmd {
            Command::ScrollDown(n) => n as f32 * line,
            Command::ScrollUp(n) => -(n as f32) * line,
            Command::HalfPageDown(n) => n as f32 * screen / 2.0,
            Command::HalfPageUp(n) => -(n as f32) * screen / 2.0,
            Command::ScreenDown(n) => n as f32 * screen,
            Command::ScreenUp(n) => -(n as f32) * screen,
            Command::Cancel | Command::Quit | Command::Hints(HintAction::Preview) => {
                self.close_link_preview();
                return None;
            }
            _ => return None,
        };
        popup.scroll(dy, total);
        None
    }

    fn preview_select(&mut self, forward: bool) -> Option<Command> {
        let popup = self.link_preview.popup.as_ref()?;
        let visible = self.visible_links(popup);
        let current = popup
            .selected
            .and_then(|s| visible.iter().position(|v| *v == s));
        let selected = step(visible.len(), current, forward).map(|i| visible[i]);
        if selected.is_none() {
            self.info("No links in the preview");
        }
        self.link_preview.popup.as_mut()?.selected = selected;
        None
    }

    /// Enter: the link picked with `<Tab>`, else where the popup's link leads.
    fn preview_follow(&mut self) -> Option<Command> {
        let popup = self.link_preview.popup.as_ref()?;
        let target = popup
            .selected
            .filter(|s| self.visible_links(popup).contains(s))
            .map(|(page, i)| self.view.links[page][i].target.clone())
            .unwrap_or_else(|| popup.target.clone());
        self.close_link_preview();
        Some(Command::FollowLink(target))
    }

    /// Whether a mouse popup stays: the pointer is on its link or on it, or
    /// was until a moment ago.
    fn popup_stays(&mut self, ctx: &egui::Context, hover: Option<&LinkTarget>) -> bool {
        if !self.preview_allowed() {
            return false;
        }
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let Some(popup) = self.link_preview.popup.as_mut() else {
            return false;
        };
        let on_popup = popup
            .rect
            .is_some_and(|r| pointer.is_some_and(|p| r.contains(p)));
        if on_popup || hover == Some(&popup.target) {
            popup.left_at = None;
            return true;
        }
        let waited = popup.left_at.get_or_insert_with(Instant::now).elapsed();
        if waited < GRACE {
            ctx.request_repaint_after(GRACE - waited);
        }
        waited < GRACE
    }

    /// The link the pointer has rested on long enough.
    fn rested_link(
        &mut self,
        ctx: &egui::Context,
        hover: Option<(LinkTarget, egui::Pos2)>,
    ) -> Option<(LinkTarget, egui::Pos2)> {
        let preview = &mut self.link_preview;
        let Some((target, pointer)) = hover else {
            preview.hovered = None;
            return None;
        };
        let since = match &preview.hovered {
            Some((t, since)) if *t == target => *since,
            _ => {
                preview.hovered = Some((target.clone(), Instant::now()));
                Instant::now()
            }
        };
        let waited = since.elapsed();
        if waited < DELAY {
            ctx.request_repaint_after(DELAY - waited);
            return None;
        }
        Some((target, pointer))
    }

    /// Open the popup on where `target` leads: below and right of the
    /// pointer (above it if there's no room below), kept inside the window,
    /// or in the middle of the window without a pointer.
    fn open_popup(&mut self, ctx: &egui::Context, target: LinkTarget, pointer: Option<egui::Pos2>) {
        let LinkTarget::Internal { page, y, .. } = target else {
            return;
        };
        let r = self.view.layout.page(page);
        let (left, page_y, w, h) = (r.x, r.y, r.w, r.h);
        let screen = ctx.content_rect();
        let zoom = WIDTH.min(screen.width() * 0.7) / w;
        let height = HEIGHT.min(screen.height() * 0.45) / zoom;
        let size = Vec2::new(w, height) * zoom;
        let outer = size + Vec2::new(16.0, 16.0 + self.config.font_size * 1.4);
        let mut pos = match pointer {
            Some(pointer) => {
                let mut pos = pointer + Vec2::new(12.0, 18.0);
                if pos.y + outer.y > screen.max.y {
                    pos.y = pointer.y - outer.y - 8.0;
                }
                pos
            }
            None => screen.center() - outer / 2.0,
        };
        pos.x = pos.x.min(screen.max.x - outer.x).max(screen.min.x);
        pos.y = pos.y.max(screen.min.y);
        let mut popup = Popup {
            target,
            left,
            top: page_y + strip_top(y, h, height),
            zoom,
            size,
            pos,
            rect: None,
            selected: None,
            left_at: None,
        };
        popup.scroll(0.0, self.view.layout.height());
        self.link_preview.popup = Some(popup);
    }

    fn popup_pages(&self, popup: &Popup) -> Range<usize> {
        self.view
            .layout
            .pages_in(popup.top, popup.top + popup.doc_height())
    }

    /// Render the popup's pages, one at a time. True when they're all in.
    fn request_pages(&mut self, ctx: &egui::Context, popup: &Popup) -> bool {
        let visible = self.popup_pages(popup);
        let scale = popup.zoom * ctx.pixels_per_point();
        let recolor = self.recolor();
        let wanted: Vec<PreviewRequest> = visible
            .clone()
            .map(|page| {
                let r = self.view.layout.page(page);
                PreviewRequest {
                    source: self.view.render_source.clone(),
                    page,
                    top: 0.0,
                    width: r.w,
                    height: r.h,
                    scale,
                    recolor,
                }
            })
            .collect();
        let preview = &mut self.link_preview;
        let previewer = preview.previewer.get_or_insert_with(|| {
            let wake = ctx.clone();
            Previewer::spawn(move || wake.request_repaint())
        });
        while let Some((req, image)) = previewer.try_recv() {
            match image {
                Ok(image) => {
                    let texture = ctx.load_texture(
                        format!("vimpdf-link-preview-{}", req.page),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    if preview.requested.as_ref() == Some(&req) {
                        preview.requested = None;
                    }
                    preview.pages.insert(req.page, (req, texture));
                }
                Err(e) => tracing::debug!("link preview failed: {e}"),
            }
        }
        preview
            .pages
            .retain(|page, _| page.abs_diff(visible.start) <= KEEP_PAGES);
        let missing = wanted
            .into_iter()
            .find(|want| preview.pages.get(&want.page).is_none_or(|(r, _)| r != want));
        let Some(want) = missing else {
            return true;
        };
        if preview.requested.as_ref() != Some(&want) {
            previewer.request(want.clone());
            preview.requested = Some(want);
        }
        false
    }

    fn draw_popup_pages(&self, ui: &egui::Ui, rect: Rect, popup: &Popup) {
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, BACKGROUND);
        let layout = &self.view.layout;
        let on_screen =
            |x: f32, y: f32| rect.min + Vec2::new(x - popup.left, y - popup.top) * popup.zoom;
        for page in self.popup_pages(popup) {
            let r = layout.page(page);
            let page_rect =
                Rect::from_min_size(on_screen(r.x, r.y), Vec2::new(r.w, r.h) * popup.zoom);
            match self.link_preview.pages.get(&page) {
                Some((_, texture)) => {
                    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    painter.image(texture.id(), page_rect, uv, Color32::WHITE);
                }
                None => {
                    painter.rect_filled(page_rect, 0.0, self.page_color());
                }
            }
        }
        if let Some((page, i)) = popup.selected
            && let Some(link) = self.view.links.get(page).and_then(|l| l.get(i))
        {
            let r = layout.page(page);
            let [red, green, blue, _] = self.config.colors.hint_background.0;
            painter.rect_filled(
                Rect::from_min_max(
                    on_screen(r.x + link.x0, r.y + link.y0),
                    on_screen(r.x + link.x1, r.y + link.y1),
                ),
                2.0,
                Color32::from_rgba_unmultiplied(red, green, blue, 110),
            );
        }
    }

    /// The links in view in the popup, as (page, index).
    fn visible_links(&self, popup: &Popup) -> Vec<(usize, usize)> {
        let (top, bottom) = (popup.top, popup.top + popup.doc_height());
        let mut found = Vec::new();
        for page in self.popup_pages(popup) {
            let r = self.view.layout.page(page);
            for (i, link) in self.view.links.get(page).into_iter().flatten().enumerate() {
                if r.y + link.y1 >= top && r.y + link.y0 <= bottom {
                    found.push((page, i));
                }
            }
        }
        found
    }

    /// The link at a point of the popup's `rect`.
    fn link_in_popup(&self, popup: &Popup, rect: Rect, pos: egui::Pos2) -> Option<&PageLink> {
        let x = popup.left + (pos.x - rect.min.x) / popup.zoom;
        let y = popup.top + (pos.y - rect.min.y) / popup.zoom;
        let layout = &self.view.layout;
        let page = layout.page_containing(x, y)?;
        let r = layout.page(page);
        let links = self.view.links.get(page)?;
        links.iter().find(|l| l.contains(x - r.x, y - r.y))
    }

    /// Whether a preview may show from the pointer: the option is on and
    /// nothing else is in the way.
    fn preview_allowed(&self) -> bool {
        self.config.link_preview
            && matches!(self.mode, Mode::Normal | Mode::Visual)
            && self.menu.is_none()
            && self.settings.is_none()
            && self.start.is_none()
    }

    /// The internal link under the pointer, when a preview may show from it,
    /// and the pointer: nothing else has the pointer and no button is held.
    fn hovered_internal_link(
        &self,
        ctx: &egui::Context,
        area: Rect,
        vp: &Viewport,
    ) -> Option<(LinkTarget, egui::Pos2)> {
        let calm = self.preview_allowed() && !ctx.egui_wants_pointer_input();
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
            LinkTarget::Internal { page, .. } if page < self.view.layout.len() => {
                Some((link.target.clone(), pointer))
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

    #[test]
    fn tab_walks_the_links_and_wraps_around() {
        assert_eq!(step(0, None, true), None);
        assert_eq!(step(3, None, true), Some(0));
        assert_eq!(step(3, None, false), Some(2));
        assert_eq!(step(3, Some(2), true), Some(0));
        assert_eq!(step(3, Some(0), false), Some(2));
    }

    #[test]
    fn popup_scrolls_inside_the_document() {
        let mut popup = Popup {
            target: LinkTarget::External(String::new()),
            left: 0.0,
            top: 100.0,
            zoom: 2.0,
            size: Vec2::new(800.0, 400.0),
            pos: egui::Pos2::ZERO,
            rect: None,
            selected: None,
            left_at: None,
        };
        popup.scroll(50.0, 1000.0);
        assert_eq!(popup.top, 150.0);
        popup.scroll(-500.0, 1000.0);
        assert_eq!(popup.top, 0.0);
        popup.scroll(5000.0, 1000.0);
        assert_eq!(
            popup.top, 800.0,
            "stops with the end of the document in view"
        );
        popup.scroll(0.0, 100.0);
        assert_eq!(popup.top, 0.0, "a document shorter than the popup");
    }
}
