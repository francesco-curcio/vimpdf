//! The visible window onto the document: scroll offset, zoom and conversions
//! between document space (PDF points) and view space (screen points relative
//! to the top-left of the view). Pure arithmetic, no MuPDF or egui.

use crate::layout::Layout;

/// Distance from the top of the view used to decide which page is "current",
/// so a page whose top just scrolled past still counts.
const CURRENT_PAGE_SLACK: f32 = 1.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Viewport {
    /// Document point at the top-left corner of the view.
    pub x: f32,
    pub y: f32,
    /// Screen points per PDF point.
    pub zoom: f32,
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

    /// Zoom so the widest page fills the view width minus `margin` on each side.
    pub fn fit_width(&mut self, layout: &Layout, margin: f32) {
        let avail = (self.width - 2.0 * margin).max(1.0);
        self.zoom = avail / layout.width().max(1.0);
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

    /// The page at the top of the view.
    pub fn current_page(&self, layout: &Layout) -> usize {
        layout.page_at_y(self.y + CURRENT_PAGE_SLACK / self.zoom)
    }

    /// Scroll so the top of `page` is at the top of the view.
    pub fn goto_page(&mut self, layout: &Layout, page: usize) {
        let page = page.min(layout.len().saturating_sub(1));
        self.y = layout.page(page).y;
    }

    /// Move `n` pages forward, snapping to the page top.
    pub fn next_page(&mut self, layout: &Layout, n: usize) {
        let target = self.current_page(layout) + n;
        self.goto_page(layout, target);
    }

    /// Move `n` pages back, snapping to the page top. If the view is partway
    /// into the current page, the first step returns to its top.
    pub fn prev_page(&mut self, layout: &Layout, n: usize) {
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
