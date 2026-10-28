//! Tile maths and render planning. Pages are rendered in square tiles so that
//! high zoom levels only render (and keep in memory) what is near the view.

use crate::document::PixelRect;
use crate::layout::Layout;
use crate::viewport::Viewport;

use super::{Recolor, RenderJob, ScaleKey};

/// Tile edge in device pixels.
pub const TILE_SIZE: u32 = 1024;

/// How far beyond the view to prefetch, in view heights.
const AHEAD_SCREENS: f32 = 2.0;
const BEHIND_SCREENS: f32 = 0.5;
/// Extra width rendered left and right of the view, in view widths.
const SIDE_SCREENS: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileId {
    pub col: u16,
    pub row: u16,
}

/// Size of a whole page in device pixels at `scale`.
pub fn page_pixels(width_pt: f32, height_pt: f32, scale: f32) -> (u32, u32) {
    (
        (width_pt * scale).ceil().max(1.0) as u32,
        (height_pt * scale).ceil().max(1.0) as u32,
    )
}

/// Pixel region of a tile, clipped to the page.
pub fn tile_rect(page_px: (u32, u32), tile: TileId) -> PixelRect {
    let x = tile.col as u32 * TILE_SIZE;
    let y = tile.row as u32 * TILE_SIZE;
    PixelRect {
        x,
        y,
        w: TILE_SIZE.min(page_px.0.saturating_sub(x)),
        h: TILE_SIZE.min(page_px.1.saturating_sub(y)),
    }
}

/// Tiles intersecting the pixel region `[x0, x1) × [y0, y1)` of a page, row
/// by row from the top.
pub fn tiles_covering(page_px: (u32, u32), x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<TileId> {
    let x0 = x0.max(0.0);
    let y0 = y0.max(0.0);
    let x1 = x1.min(page_px.0 as f32);
    let y1 = y1.min(page_px.1 as f32);
    if x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    let t = TILE_SIZE as f32;
    let cols = (x0 / t) as u16..=((x1 - 1e-3) / t) as u16;
    let rows = (y0 / t) as u16..=((y1 - 1e-3) / t) as u16;
    rows.flat_map(|row| cols.clone().map(move |col| TileId { col, row }))
        .collect()
}

/// A rectangle in document space (PDF points).
#[derive(Debug, Clone, Copy)]
struct Region {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

/// Tiles of every page intersecting `region`, pages in order (reversed when
/// `reverse`, so the nearest page comes first while scrolling up).
fn tiles_in(
    layout: &Layout,
    generations: &[u32],
    scale: ScaleKey,
    recolor: Option<Recolor>,
    region: Region,
    reverse: bool,
) -> Vec<RenderJob> {
    let s = scale.scale();
    let mut pages: Vec<usize> = layout.pages_in(region.y0, region.y1).collect();
    if reverse {
        pages.reverse();
    }
    let mut jobs = Vec::new();
    for page in pages {
        let r = layout.page(page);
        let page_px = page_pixels(r.w, r.h, s);
        let mut tiles = tiles_covering(
            page_px,
            (region.x0 - r.x) * s,
            (region.y0 - r.y) * s,
            (region.x1 - r.x) * s,
            (region.y1 - r.y) * s,
        );
        if reverse {
            tiles.reverse();
        }
        jobs.extend(tiles.into_iter().map(|tile| RenderJob {
            page,
            generation: generations.get(page).copied().unwrap_or(0),
            scale,
            recolor,
            tile,
            rect: tile_rect(page_px, tile),
        }));
    }
    jobs
}

/// Tiles visible in `vp`, in drawing order.
pub fn visible_tiles(
    layout: &Layout,
    generations: &[u32],
    vp: &Viewport,
    scale: ScaleKey,
    recolor: Option<Recolor>,
) -> Vec<RenderJob> {
    let view = Region {
        x0: vp.x,
        y0: vp.y,
        x1: vp.x + vp.doc_width(),
        y1: vp.y + vp.doc_height(),
    };
    tiles_in(layout, generations, scale, recolor, view, false)
}

/// Everything worth rendering, highest priority first: the visible tiles,
/// then a margin to the sides, then ahead in the scroll direction, then behind.
pub fn plan(
    layout: &Layout,
    generations: &[u32],
    vp: &Viewport,
    scale: ScaleKey,
    recolor: Option<Recolor>,
    scrolling_up: bool,
) -> Vec<RenderJob> {
    let (w, h) = (vp.doc_width(), vp.doc_height());
    let (x0, x1) = (vp.x - SIDE_SCREENS * w, vp.x + w + SIDE_SCREENS * w);
    let (top, bottom) = (vp.y, vp.y + h);

    let below = |screens: f32| Region {
        x0,
        y0: bottom,
        x1,
        y1: bottom + screens * h,
    };
    let above = |screens: f32| Region {
        x0,
        y0: top - screens * h,
        x1,
        y1: top,
    };

    let mut jobs = visible_tiles(layout, generations, vp, scale, recolor);
    let sides = Region {
        x0,
        y0: top,
        x1,
        y1: bottom,
    };
    jobs.extend(tiles_in(layout, generations, scale, recolor, sides, false));
    if scrolling_up {
        jobs.extend(tiles_in(
            layout,
            generations,
            scale,
            recolor,
            above(AHEAD_SCREENS),
            true,
        ));
        jobs.extend(tiles_in(
            layout,
            generations,
            scale,
            recolor,
            below(BEHIND_SCREENS),
            false,
        ));
    } else {
        jobs.extend(tiles_in(
            layout,
            generations,
            scale,
            recolor,
            below(AHEAD_SCREENS),
            false,
        ));
        jobs.extend(tiles_in(
            layout,
            generations,
            scale,
            recolor,
            above(BEHIND_SCREENS),
            true,
        ));
    }

    let mut seen = std::collections::HashSet::new();
    jobs.retain(|j| seen.insert(*j));
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::PageSize;

    fn t(col: u16, row: u16) -> TileId {
        TileId { col, row }
    }

    #[test]
    fn tile_rects_are_clipped_to_the_page() {
        let px = (2500, 1100);
        assert_eq!(
            tile_rect(px, t(0, 0)),
            PixelRect {
                x: 0,
                y: 0,
                w: 1024,
                h: 1024
            }
        );
        assert_eq!(
            tile_rect(px, t(2, 1)),
            PixelRect {
                x: 2048,
                y: 1024,
                w: 452,
                h: 76
            }
        );
    }

    #[test]
    fn tiles_covering_region() {
        let px = (2500, 3000);
        assert_eq!(tiles_covering(px, 0.0, 0.0, 1024.0, 1024.0), vec![t(0, 0)]);
        assert_eq!(
            tiles_covering(px, 1000.0, 1000.0, 1100.0, 1100.0),
            vec![t(0, 0), t(1, 0), t(0, 1), t(1, 1)]
        );
        assert_eq!(
            tiles_covering(px, -500.0, 2900.0, 9999.0, 9999.0),
            vec![t(0, 2), t(1, 2), t(2, 2)]
        );
        assert!(tiles_covering(px, 0.0, 3000.0, 100.0, 3100.0).is_empty());
    }

    #[test]
    fn plan_puts_visible_tiles_first_then_ahead() {
        let layout = Layout::new(&[PageSize::new(500.0, 700.0); 10], 10.0);
        let mut vp = Viewport::new(520.0, 400.0);
        vp.fit_width(&layout, 10.0);
        vp.clamp(&layout);
        vp.goto_page(&layout, 3);

        let scale = ScaleKey::from_scale(2.0);
        let visible = visible_tiles(&layout, &[], &vp, scale, None);
        assert!(visible.iter().all(|j| j.page == 3));

        let down = plan(&layout, &[], &vp, scale, None, false);
        assert_eq!(&down[..visible.len()], &visible[..]);
        let first_other = down.iter().find(|j| j.page != 3).unwrap();
        assert_eq!(first_other.page, 4, "scrolling down prefetches below first");

        let up = plan(&layout, &[], &vp, scale, None, true);
        let first_other = up.iter().find(|j| j.page != 3).unwrap();
        assert_eq!(first_other.page, 2, "scrolling up prefetches above first");

        let mut dedup = down.clone();
        dedup.dedup();
        assert_eq!(dedup.len(), down.len());
    }
}
