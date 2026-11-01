//! The vertical scrollbar on the right of the pages.

use eframe::egui::{self, Color32, CursorIcon, Rect, Sense, Vec2};

use super::BAR_DIM;

/// Width of the strip the scrollbar takes beside the pages.
pub const WIDTH: f32 = 12.0;
/// The thumb never gets shorter than this, even in long documents.
const MIN_THUMB: f32 = 24.0;

const TRACK: Color32 = Color32::from_gray(32);
const THUMB: Color32 = Color32::from_gray(90);

/// What the user did with the scrollbar this frame.
pub struct Scrolled {
    /// The new top of the view, in document points.
    pub y: Option<f32>,
    /// The thumb is being dragged (to show the page number beside it).
    pub dragging: bool,
    /// Where the thumb is drawn.
    pub thumb: Rect,
}

/// Draw the scrollbar in `rect` for a view showing `view` of `total`
/// document points from `y` down. Dragging the thumb scrolls; pressing the
/// track moves the thumb's centre there (and keeps dragging from there).
pub fn show(ui: &mut egui::Ui, rect: Rect, y: f32, view: f32, total: f32) -> Scrolled {
    let geometry = Geometry::new(rect, view, total);
    let thumb = geometry.thumb(y);
    let response = ui.interact(
        rect,
        egui::Id::unique("vimpdf-scrollbar"),
        Sense::click_and_drag(),
    );

    let mut new_y = None;
    let pointer = response.interact_pointer_pos();
    if let Some(p) = pointer.filter(|_| response.drag_started() || response.clicked())
        && !thumb.contains(p)
    {
        new_y = Some(geometry.y_centred_at(p.y));
    } else if response.dragged() {
        new_y = Some(y + geometry.doc_per_pixel() * response.drag_delta().y);
    }
    let thumb = new_y.map_or(thumb, |y| geometry.thumb(y));

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, TRACK);
    let active = response.hovered() || response.dragged();
    let fill = if active { BAR_DIM } else { THUMB };
    let inset = if active { 2.0 } else { 3.0 };
    painter.rect_filled(thumb.shrink2(Vec2::new(inset, 1.0)), 3.0, fill);
    if active {
        ui.ctx().set_cursor_icon(CursorIcon::Default);
    }
    Scrolled {
        y: new_y,
        dragging: response.dragged(),
        thumb,
    }
}

/// Mapping between document positions and the thumb.
struct Geometry {
    rect: Rect,
    thumb_h: f32,
    /// Document points the view can scroll through.
    scrollable: f32,
    /// Pixels the thumb can move through.
    free: f32,
}

impl Geometry {
    fn new(rect: Rect, view: f32, total: f32) -> Self {
        let total = total.max(1.0);
        let thumb_h = (rect.height() * (view / total).min(1.0))
            .max(MIN_THUMB)
            .min(rect.height());
        Self {
            rect,
            thumb_h,
            scrollable: (total - view).max(0.0),
            free: (rect.height() - thumb_h).max(0.0),
        }
    }

    fn thumb(&self, y: f32) -> Rect {
        let t = if self.scrollable > 0.0 {
            (y / self.scrollable).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Rect::from_min_size(
            egui::pos2(self.rect.min.x, self.rect.min.y + t * self.free),
            Vec2::new(self.rect.width(), self.thumb_h),
        )
    }

    fn doc_per_pixel(&self) -> f32 {
        if self.free > 0.0 {
            self.scrollable / self.free
        } else {
            0.0
        }
    }

    /// The view top that puts the thumb's centre at screen height `py`.
    fn y_centred_at(&self, py: f32) -> f32 {
        let top = py - self.rect.min.y - self.thumb_h / 2.0;
        (top * self.doc_per_pixel()).clamp(0.0, self.scrollable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> Geometry {
        // A 1000 px track, a view of 1000 of 10_000 points: 100 px thumb.
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(WIDTH, 1000.0));
        Geometry::new(rect, 1000.0, 10_000.0)
    }

    #[test]
    fn thumb_follows_the_view() {
        let g = geometry();
        assert_eq!(g.thumb(0.0).min.y, 0.0);
        assert_eq!(g.thumb(0.0).height(), 100.0);
        assert_eq!(g.thumb(9000.0).max.y, 1000.0, "at the end");
        assert_eq!(g.thumb(4500.0).min.y, 450.0);
    }

    #[test]
    fn pressing_the_track_centres_the_thumb() {
        let g = geometry();
        assert_eq!(g.y_centred_at(500.0), 4500.0);
        assert_eq!(g.y_centred_at(10.0), 0.0, "clamped at the top");
        assert_eq!(g.y_centred_at(990.0), 9000.0, "clamped at the end");
    }

    #[test]
    fn long_documents_keep_a_usable_thumb_and_short_ones_fill_the_track() {
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(WIDTH, 500.0));
        assert_eq!(Geometry::new(rect, 100.0, 1_000_000.0).thumb_h, MIN_THUMB);
        let short = Geometry::new(rect, 800.0, 600.0);
        assert_eq!(short.thumb_h, 500.0);
        assert_eq!(short.doc_per_pixel(), 0.0);
    }
}
