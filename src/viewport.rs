//! The visible window onto the document: scroll offset, zoom and conversions
//! between document space (PDF points) and view space (screen points relative
//! to the top-left of the view). Pure arithmetic, no MuPDF or egui.

use crate::layout::Layout;

/// Distance from the top of the view used to decide which page is "current",
/// so a page whose top just scrolled past still counts.
const CURRENT_PAGE_SLACK: f32 = 1.0;
/// Zoom limits, in screen points per PDF point (0.1 = 10%, 10 = 1000%).
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 10.0;

/// How the zoom follows the window size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// The widest page fills the window width.
    Width,
    /// The current page fits entirely in the window.
    Page,
    /// Zoom stays where the user put it.
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Viewport {
    /// Document point at the top-left corner of the view.
    pub x: f32,
    pub y: f32,
    /// Screen points per PDF point.
    pub zoom: f32,
    pub fit: FitMode,
    /// View size in screen points.
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
            fit: FitMode::Width,
            width,
            height,
        }
    }

    /// Visible height in document points.
    pub fn doc_height(&self) -> f32 {
        self.height / self.zoom
    }

    pub fn doc_width(&self) -> f32 {
        self.width / self.zoom
    }

    pub fn to_view(&self, x: f32, y: f32) -> (f32, f32) {
        ((x - self.x) * self.zoom, (y - self.y) * self.zoom)
    }

    pub fn to_doc(&self, vx: f32, vy: f32) -> (f32, f32) {
        (self.x + vx / self.zoom, self.y + vy / self.zoom)
    }

    /// Resize the view, keeping the document point at the top of the view fixed.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    /// Zoom so the widest page fills the view width minus `margin` on each
    /// side, keeping the top of the view where it is.
    pub fn fit_width(&mut self, layout: &Layout, margin: f32) {
        let avail = (self.width - 2.0 * margin).max(1.0);
        self.zoom = (avail / layout.width().max(1.0)).clamp(MIN_ZOOM, MAX_ZOOM);
        self.fit = FitMode::Width;
    }

    /// Zoom so the page at the centre of the view fits entirely, and centre it.
    pub fn fit_page(&mut self, layout: &Layout, margin: f32) {
        let page = self.centre_page(layout);
        let r = layout.page(page);
        let avail_w = (self.width - 2.0 * margin).max(1.0);
        let avail_h = (self.height - 2.0 * margin).max(1.0);
        self.zoom = (avail_w / r.w).min(avail_h / r.h).clamp(MIN_ZOOM, MAX_ZOOM);
        self.y = r.y - (self.doc_height() - r.h) / 2.0;
        self.fit = FitMode::Page;
    }

    /// Re-apply the fit mode, e.g. after a resize.
    pub fn refit(&mut self, layout: &Layout, margin: f32) {
        match self.fit {
            FitMode::Width => self.fit_width(layout, margin),
            FitMode::Page => self.fit_page(layout, margin),
            FitMode::None => {}
        }
    }

    /// Set the zoom keeping the document point under the view point
    /// (`ax`, `ay`) fixed. Leaves fit mode.
    pub fn zoom_to(&mut self, zoom: f32, ax: f32, ay: f32) {
        let (dx, dy) = self.to_doc(ax, ay);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.x = dx - ax / self.zoom;
        self.y = dy - ay / self.zoom;
        self.fit = FitMode::None;
    }

    /// Multiply the zoom, anchored at the view point (`ax`, `ay`).
    pub fn zoom_by(&mut self, factor: f32, ax: f32, ay: f32) {
        self.zoom_to(self.zoom * factor, ax, ay);
    }

    /// Keep the view inside the document. Content narrower than the view is
    /// centred horizontally.
    pub fn clamp(&mut self, layout: &Layout) {
        let max_y = (layout.height() - self.doc_height()).max(0.0);
        self.y = self.y.clamp(0.0, max_y);

        let doc_w = self.doc_width();
        if layout.width() <= doc_w {
            self.x = (layout.width() - doc_w) / 2.0;
        } else {
            self.x = self.x.clamp(0.0, layout.width() - doc_w);
        }
    }

    /// Scroll by a distance in screen points.
    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        self.x += dx / self.zoom;
        self.y += dy / self.zoom;
    }

    /// The page at the top of the view. If the top edge is in the gap below
    /// a page, the page after the gap.
    pub fn current_page(&self, layout: &Layout) -> usize {
        let y = self.y + CURRENT_PAGE_SLACK / self.zoom;
        let i = layout.page_at_y(y);
        if y > layout.page(i).bottom() && i + 1 < layout.len() {
            i + 1
        } else {
            i
        }
    }

    /// The page at the vertical centre of the view.
    pub fn centre_page(&self, layout: &Layout) -> usize {
        layout.page_at_y(self.y + self.doc_height() / 2.0)
    }

    /// Scroll so the top of `page` is at the top of the view (in fit-page
    /// mode: so the page is centred).
    pub fn goto_page(&mut self, layout: &Layout, page: usize) {
        let r = layout.page(page.min(layout.len().saturating_sub(1)));
        self.y = match self.fit {
            FitMode::Page => r.y - (self.doc_height() - r.h) / 2.0,
            _ => r.y,
        };
    }

    /// Move `n` pages forward, snapping to the page top.
    pub fn next_page(&mut self, layout: &Layout, n: usize) {
        let current = match self.fit {
            FitMode::Page => self.centre_page(layout),
            _ => self.current_page(layout),
        };
        self.goto_page(layout, current + n);
    }

    /// Move `n` pages back, snapping to the page top. If the view is partway
    /// into the current page, the first step returns to its top.
    pub fn prev_page(&mut self, layout: &Layout, n: usize) {
        if self.fit == FitMode::Page {
            let current = self.centre_page(layout);
            self.goto_page(layout, current.saturating_sub(n));
            return;
        }
        let current = self.current_page(layout);
        let at_top = self.y - layout.page(current).y <= CURRENT_PAGE_SLACK / self.zoom;
        let steps = if at_top { n } else { n - 1 };
        self.goto_page(layout, current.saturating_sub(steps));
    }

    pub fn goto_end(&mut self, layout: &Layout) {
        self.y = layout.height();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::PageSize;

    fn layout() -> Layout {
        Layout::new(&[PageSize::new(500.0, 700.0); 5], 10.0)
    }

    fn view() -> Viewport {
        let mut v = Viewport::new(520.0, 400.0);
        v.fit_width(&layout(), 10.0);
        v
    }

    #[test]
    fn fit_width_and_conversions() {
        let v = view();
        assert_eq!(v.zoom, 1.0);
        let mut v2 = v.clone();
        v2.zoom = 2.0;
        v2.x = 10.0;
        v2.y = 100.0;
        assert_eq!(v2.to_view(20.0, 150.0), (20.0, 100.0));
        assert_eq!(v2.to_doc(20.0, 100.0), (20.0, 150.0));
    }

    #[test]
    fn clamp_keeps_view_in_document_and_centres_narrow_content() {
        let l = layout();
        let mut v = view();
        v.y = -50.0;
        v.clamp(&l);
        assert_eq!(v.y, 0.0);
        assert_eq!(v.x, -10.0);

        v.y = 1e6;
        v.clamp(&l);
        assert_eq!(v.y, l.height() - 400.0);
    }

    #[test]
    fn clamp_allows_horizontal_scroll_when_zoomed_in() {
        let l = layout();
        let mut v = view();
        v.zoom = 2.0;
        v.x = 1e6;
        v.clamp(&l);
        assert_eq!(v.x, 500.0 - 260.0);
    }

    #[test]
    fn zoom_keeps_anchor_point_fixed() {
        let l = layout();
        let mut v = view();
        v.y = 300.0;
        let anchor = (100.0, 200.0);
        let before = v.to_doc(anchor.0, anchor.1);
        v.zoom_by(2.0, anchor.0, anchor.1);
        assert_eq!(v.zoom, 2.0);
        assert_eq!(v.fit, FitMode::None);
        let after = v.to_doc(anchor.0, anchor.1);
        assert!((before.0 - after.0).abs() < 1e-3 && (before.1 - after.1).abs() < 1e-3);

        v.zoom_by(1000.0, 0.0, 0.0);
        assert_eq!(v.zoom, MAX_ZOOM);
        v.zoom_by(0.0001, 0.0, 0.0);
        assert_eq!(v.zoom, MIN_ZOOM);
        v.clamp(&l);
    }

    #[test]
    fn fit_page_shows_whole_current_page_centred() {
        let l = layout();
        let mut v = view();
        v.goto_page(&l, 2);
        v.fit_page(&l, 10.0);
        assert_eq!(v.fit, FitMode::Page);
        // 700pt tall page in 380pt of usable height.
        assert!((v.zoom - 380.0 / 700.0).abs() < 1e-5);
        let r = l.page(2);
        let (_, top) = v.to_view(r.x, r.y);
        let (_, bottom) = v.to_view(r.x, r.bottom());
        assert!(
            (top - (400.0 - bottom)).abs() < 1e-3,
            "page is centred vertically"
        );
        assert_eq!(v.centre_page(&l), 2);

        // Re-fitting (e.g. on resize) must not drift to another page.
        v.resize(520.0, 500.0);
        v.refit(&l, 10.0);
        v.refit(&l, 10.0);
        assert_eq!(v.centre_page(&l), 2);

        // Page motions keep pages centred.
        v.next_page(&l, 1);
        assert_eq!(v.centre_page(&l), 3);
        v.prev_page(&l, 2);
        assert_eq!(v.centre_page(&l), 1);
    }

    #[test]
    fn refit_follows_mode() {
        let l = layout();
        let mut v = view();
        v.resize(1020.0, 400.0);
        v.refit(&l, 10.0);
        assert_eq!(v.zoom, 2.0);

        v.zoom_by(1.5, 0.0, 0.0);
        v.resize(520.0, 400.0);
        v.refit(&l, 10.0);
        assert_eq!(v.zoom, 3.0, "free zoom survives a resize");
    }

    #[test]
    fn view_top_in_a_gap_belongs_to_the_next_page() {
        let l = layout();
        let mut v = view();
        v.y = l.page(1).bottom() + 5.0;
        assert_eq!(v.current_page(&l), 2);
    }

    #[test]
    fn page_navigation_snaps_to_page_tops() {
        let l = layout();
        let mut v = view();
        v.next_page(&l, 1);
        assert_eq!(v.y, l.page(1).y);
        assert_eq!(v.current_page(&l), 1);

        v.next_page(&l, 2);
        assert_eq!(v.current_page(&l), 3);

        v.scroll_by(0.0, 100.0);
        v.prev_page(&l, 1);
        assert_eq!(v.y, l.page(3).y, "first K returns to top of current page");
        v.prev_page(&l, 1);
        assert_eq!(v.y, l.page(2).y);

        v.next_page(&l, 100);
        assert_eq!(v.current_page(&l), 4);
        v.prev_page(&l, 100);
        assert_eq!(v.y, l.page(0).y);
    }
}
