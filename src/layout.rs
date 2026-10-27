//! Continuous vertical layout: every page gets a rectangle in document space
//! (PDF points). Computed once at open time; scrolling and zooming never call
//! MuPDF.

use std::ops::Range;

use crate::document::PageSize;

/// Rectangle in document space, in PDF points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DocRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl DocRect {
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.bottom()
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    pages: Vec<DocRect>,
    width: f32,
    height: f32,
}

impl Layout {
    /// Stack pages vertically with `gap` points between them (and above the
    /// first / below the last), centred horizontally on the widest page.
    pub fn new(sizes: &[PageSize], gap: f32) -> Self {
        let width = sizes.iter().map(|s| s.width).fold(0.0, f32::max);
        let mut y = gap;
        let pages = sizes
            .iter()
            .map(|s| {
                let rect = DocRect {
                    x: (width - s.width) / 2.0,
                    y,
                    w: s.width,
                    h: s.height,
                };
                y += s.height + gap;
                rect
            })
            .collect();
        Self {
            pages,
            width,
            height: y,
        }
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn page(&self, i: usize) -> DocRect {
        self.pages[i]
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    /// The page at vertical position `y`. A position in the gap between two
    /// pages belongs to the page above it.
    pub fn page_at_y(&self, y: f32) -> usize {
        let after = self.pages.partition_point(|p| p.y <= y);
        after.saturating_sub(1)
    }

    /// Pages intersecting the vertical span `[y0, y1]`.
    pub fn pages_in(&self, y0: f32, y1: f32) -> Range<usize> {
        let start = self.pages.partition_point(|p| p.bottom() < y0);
        let end = self.pages.partition_point(|p| p.y <= y1);
        start..end.max(start)
    }

    /// The page containing the document point, if any.
    pub fn page_containing(&self, x: f32, y: f32) -> Option<usize> {
        let i = self.page_at_y(y);
        self.pages[i].contains(x, y).then_some(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes() -> Vec<PageSize> {
        vec![
            PageSize::new(600.0, 800.0),
            PageSize::new(400.0, 500.0),
            PageSize::new(600.0, 800.0),
        ]
    }

    #[test]
    fn stacks_pages_with_gaps_and_centres_them() {
        let l = Layout::new(&sizes(), 10.0);
        assert_eq!(l.width(), 600.0);
        assert_eq!(l.page(0).y, 10.0);
        assert_eq!(l.page(1).y, 820.0);
        assert_eq!(l.page(1).x, 100.0);
        assert_eq!(l.page(2).y, 1330.0);
        assert_eq!(l.height(), 2140.0);
    }

    #[test]
    fn page_at_y_assigns_gaps_to_page_above() {
        let l = Layout::new(&sizes(), 10.0);
        assert_eq!(l.page_at_y(0.0), 0);
        assert_eq!(l.page_at_y(815.0), 0);
        assert_eq!(l.page_at_y(820.0), 1);
        assert_eq!(l.page_at_y(5000.0), 2);
    }

    #[test]
    fn pages_in_returns_intersecting_range() {
        let l = Layout::new(&sizes(), 10.0);
        assert_eq!(l.pages_in(0.0, 100.0), 0..1);
        assert_eq!(l.pages_in(700.0, 900.0), 0..2);
        assert_eq!(l.pages_in(812.0, 818.0), 1..1);
        assert_eq!(l.pages_in(0.0, 5000.0), 0..3);
    }

    #[test]
    fn page_containing_respects_horizontal_bounds() {
        let l = Layout::new(&sizes(), 10.0);
        assert_eq!(l.page_containing(300.0, 1000.0), Some(1));
        assert_eq!(l.page_containing(50.0, 1000.0), None);
        assert_eq!(l.page_containing(300.0, 815.0), None);
    }
}
