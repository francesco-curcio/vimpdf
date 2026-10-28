//! Placing a box on the page, from the keyboard or with the mouse: a
//! redaction area (`X`), a sticky note (`C`), a text box (`T`), or moving one
//! of these (`m` in the annotations panel, or the right-click menu).

use eframe::egui::{Pos2, Stroke, StrokeKind};

use super::*;
use crate::annotate::{AnnotKind, EditRequest};
use crate::text::TextRect;

/// Movement of one `h`/`j`/`k`/`l`, in screen points.
const STEP: f32 = 10.0;
/// Size of a sticky note's icon, in page points.
pub(super) const NOTE_SIZE: f32 = 20.0;
/// Text box font sizes `+` / `-` step through.
pub(super) const TEXT_SIZES: &[f32] = &[
    6.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 40.0, 48.0, 60.0,
    72.0,
];
pub(super) const REDACT_RED: Color32 = Color32::from_rgb(220, 40, 40);
const PLACE_BLUE: Color32 = Color32::from_rgb(51, 154, 240);

/// What the box becomes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Purpose {
    Redact,
    Note {
        color: [u8; 3],
    },
    Text {
        size: f32,
        color: [u8; 3],
    },
    /// Moving an existing annotation (it stays on its page).
    Move {
        xref: i32,
        kind: AnnotKind,
    },
}

impl Purpose {
    pub(super) fn label(self) -> &'static str {
        match self {
            Purpose::Redact => "REDACT AREA",
            Purpose::Note { .. } => "NOTE",
            Purpose::Text { .. } => "TEXT BOX",
            Purpose::Move { .. } => "MOVE",
        }
    }

    /// Which sides `H J K L` (and a mouse drag) may change: width, height.
    /// A note is an icon; a text box is as tall as its text.
    fn resizable(self) -> (bool, bool) {
        match self {
            Purpose::Redact
            | Purpose::Move {
                kind: AnnotKind::Redact,
                ..
            } => (true, true),
            Purpose::Text { .. }
            | Purpose::Move {
                kind: AnnotKind::Text,
                ..
            } => (true, false),
            Purpose::Note { .. } | Purpose::Move { .. } => (false, false),
        }
    }

    fn color(self) -> Color32 {
        match self {
            Purpose::Redact
            | Purpose::Move {
                kind: AnnotKind::Redact,
                ..
            } => REDACT_RED,
            _ => PLACE_BLUE,
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Purpose::Redact => "Enter marks · Esc cancels",
            Purpose::Note { .. } => "Enter writes the note · Esc cancels",
            Purpose::Text { .. } => "Enter writes the text · +/- size · Esc cancels",
            Purpose::Move { .. } => "Enter moves it here · Esc cancels",
        }
    }
}

/// A mouse drag in placement mode.
#[derive(Debug, Clone, Copy)]
enum PlaceDrag {
    /// Drawing a new box from this corner (page coordinates).
    Draw { from: (f32, f32) },
    /// Moving the box, held at this offset from its top-left corner.
    Move { grab: (f32, f32) },
}

/// The box being placed, in page coordinates.
pub(super) struct Placing {
    pub purpose: Purpose,
    page: usize,
    rect: TextRect,
    /// Count typed before a key (`5l`).
    count: u32,
    drag: Option<PlaceDrag>,
}

/// The height of a text box being placed: one line.
fn one_line(size: f32) -> f32 {
    size * 1.4
}

impl VimpdfApp {
    /// `X`, `C`, `T`: a box in the middle of the view.
    pub(super) fn start_placing(&mut self, purpose: Purpose) {
        let Some(vp) = &self.viewport else { return };
        let layout = &self.view.layout;
        let page = vp.centre_page(layout);
        let r = layout.page(page);
        let (cx, cy) = vp.to_doc(vp.width / 2.0, vp.height / 2.0);
        let (w, h) = match purpose {
            Purpose::Note { .. } => (NOTE_SIZE, NOTE_SIZE),
            Purpose::Text { size, .. } => ((r.w / 3.0).min(200.0), one_line(size)),
            _ => ((r.w / 3.0).min(200.0), (r.h / 8.0).min(100.0)),
        };
        let x0 = (cx - r.x - w / 2.0).clamp(0.0, (r.w - w).max(0.0));
        let y0 = (cy - r.y - h / 2.0).clamp(0.0, (r.h - h).max(0.0));
        let rect = TextRect {
            x0,
            y0,
            x1: x0 + w,
            y1: y0 + h,
        };
        self.begin_placing(purpose, page, rect);
    }

    /// Move an existing note, text box or redaction mark.
    pub(super) fn start_moving(&mut self, page: usize, xref: i32) {
        let Some(a) = self
            .annots
            .list
            .iter()
            .find(|a| a.page == page && a.xref == xref)
        else {
            return self.error("Annotation not found");
        };
        if !a.kind.movable() || a.rects.len() != 1 {
            return self.error(format!("A {} can't be moved", a.kind.name()));
        }
        let (kind, rect) = (a.kind, a.rects[0]);
        self.mode = Mode::Normal;
        self.begin_placing(Purpose::Move { xref, kind }, page, rect);
    }

    fn begin_placing(&mut self, purpose: Purpose, page: usize, rect: TextRect) {
        self.placing = Some(Placing {
            purpose,
            page,
            rect,
            count: 0,
            drag: None,
        });
        self.keys.reset();
        self.mode = Mode::Place;
        let resize = match purpose.resizable() {
            (true, true) => ", H J K L resize",
            (true, false) => ", H L change the width",
            _ => "",
        };
        let size = if matches!(purpose, Purpose::Text { .. }) {
            ", + - font size"
        } else {
            ""
        };
        self.info(format!(
            "{}: h j k l move{resize}{size} (counts work), Enter confirms, Esc cancels; or use the mouse",
            purpose.label()
        ));
    }

    /// A key in placement mode.
    pub(super) fn place_key(&mut self, tok: &str) {
        let zoom = self.viewport.as_ref().map_or(1.0, |vp| vp.zoom);
        let Some(pl) = self.placing.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        if let Some(d) = tok.chars().next().and_then(|c| c.to_digit(10))
            && tok.len() == 1
            && (d > 0 || pl.count > 0)
        {
            pl.count = (pl.count * 10 + d).min(999);
            return;
        }
        let n = pl.count.max(1) as f32 * STEP / zoom;
        pl.count = 0;
        let page = self.view.layout.page(pl.page);
        let (resize_w, resize_h) = pl.purpose.resizable();
        let r = &mut pl.rect;
        let min = 4.0;
        match tok {
            "h" | "<Left>" => {
                let d = n.min(r.x0);
                r.x0 -= d;
                r.x1 -= d;
            }
            "l" | "<Right>" => {
                let d = n.min(page.w - r.x1);
                r.x0 += d;
                r.x1 += d;
            }
            "k" | "<Up>" => {
                let d = n.min(r.y0);
                r.y0 -= d;
                r.y1 -= d;
            }
            "j" | "<Down>" => {
                let d = n.min(page.h - r.y1);
                r.y0 += d;
                r.y1 += d;
            }
            "L" if resize_w => r.x1 = (r.x1 + n).min(page.w),
            "H" if resize_w => r.x1 = (r.x1 - n).max(r.x0 + min),
            "J" if resize_h => r.y1 = (r.y1 + n).min(page.h),
            "K" if resize_h => r.y1 = (r.y1 - n).max(r.y0 + min),
            "+" | "-" => {
                if let Purpose::Text { size, color } = pl.purpose {
                    let size = step_size(size, tok == "+");
                    pl.purpose = Purpose::Text { size, color };
                    r.y1 = r.y0 + one_line(size);
                }
            }
            "<CR>" => self.finish_placing(),
            "<Esc>" | "q" => {
                self.placing = None;
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    /// Enter (or a double click): turn the box into what it's for.
    fn finish_placing(&mut self) {
        let Some(pl) = self.placing.take() else {
            return;
        };
        self.mode = Mode::Normal;
        let (page, rect) = (pl.page, pl.rect);
        match pl.purpose {
            Purpose::Redact => self.request_marks(vec![(page, vec![rect])]),
            Purpose::Note { color } => self.write_new_note(page, rect, color),
            Purpose::Text { size, color } => self.write_new_text(page, rect, size, color),
            Purpose::Move { xref, .. } => {
                self.view
                    .editor
                    .request(EditRequest::SetRect { page, xref, rect });
            }
        }
    }

    /// The mouse in placement mode: drag the box to move it, drag elsewhere
    /// to draw a new one (or, for a note, click to put it there); a double
    /// click confirms.
    pub(super) fn place_mouse(&mut self, ctx: &egui::Context, area_rect: Rect) {
        let Some(vp) = self.viewport.clone() else {
            return;
        };
        let (pressed, down, double, pos) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer
                    .button_double_clicked(egui::PointerButton::Primary),
                i.pointer.interact_pos(),
            )
        });
        let Some(pos) = pos.filter(|p| area_rect.contains(*p)) else {
            return;
        };
        let rel = pos - area_rect.min;
        let (x, y) = vp.to_doc(rel.x, rel.y);
        let layout = &self.view.layout;
        let Some(pl) = self.placing.as_mut() else {
            return;
        };
        let (resize_w, _) = pl.purpose.resizable();
        let moving = matches!(pl.purpose, Purpose::Move { .. });
        if pressed && let Some(page) = layout.page_containing(x, y) {
            let r = layout.page(page);
            let (px, py) = (x - r.x, y - r.y);
            let inside = page == pl.page
                && px >= pl.rect.x0
                && px <= pl.rect.x1
                && py >= pl.rect.y0
                && py <= pl.rect.y1;
            if inside {
                pl.drag = Some(PlaceDrag::Move {
                    grab: (px - pl.rect.x0, py - pl.rect.y0),
                });
            } else if !moving || page == pl.page {
                pl.page = page;
                if resize_w {
                    pl.drag = Some(PlaceDrag::Draw { from: (px, py) });
                } else {
                    let (w, h) = (pl.rect.x1 - pl.rect.x0, pl.rect.y1 - pl.rect.y0);
                    pl.drag = Some(PlaceDrag::Move {
                        grab: (w / 2.0, h / 2.0),
                    });
                }
            }
        }
        let page = layout.page(pl.page);
        let (px, py) = (
            (x - page.x).clamp(0.0, page.w),
            (y - page.y).clamp(0.0, page.h),
        );
        match pl.drag {
            Some(PlaceDrag::Move { grab }) => {
                let (w, h) = (pl.rect.x1 - pl.rect.x0, pl.rect.y1 - pl.rect.y0);
                let x0 = (px - grab.0).clamp(0.0, (page.w - w).max(0.0));
                let y0 = (py - grab.1).clamp(0.0, (page.h - h).max(0.0));
                pl.rect = TextRect {
                    x0,
                    y0,
                    x1: x0 + w,
                    y1: y0 + h,
                };
            }
            Some(PlaceDrag::Draw { from: (fx, fy) }) if (px - fx).abs() > 2.0 => {
                let h = pl.rect.y1 - pl.rect.y0;
                let (y0, y1) = match pl.purpose.resizable() {
                    (_, true) if (py - fy).abs() > 2.0 => (fy.min(py), fy.max(py)),
                    (_, true) => (pl.rect.y0, pl.rect.y1),
                    // Text: the drag sets the width; the height is one line.
                    _ => (fy, fy + h),
                };
                pl.rect = TextRect {
                    x0: fx.min(px),
                    y0,
                    x1: fx.max(px),
                    y1,
                };
            }
            _ => {}
        }
        if !down {
            pl.drag = None;
        }
        if double {
            self.finish_placing();
        }
    }

    /// The box being placed.
    pub(super) fn draw_placing(&self, painter: &egui::Painter, area: Rect, vp: &Viewport) {
        let Some(pl) = &self.placing else { return };
        let r = self.page_rect_to_screen(area, vp, pl.page, &pl.rect);
        let color = pl.purpose.color();
        let [cr, cg, cb, _] = color.to_array();
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(cr, cg, cb, 50));
        painter.rect_stroke(r, 0.0, Stroke::new(2.0, color), StrokeKind::Outside);
        match pl.purpose {
            Purpose::Note {
                color: [nr, ng, nb],
            } => {
                note_icon(painter, r, Color32::from_rgb(nr, ng, nb));
            }
            Purpose::Text {
                size,
                color: [tr, tg, tb],
            } => {
                painter.text(
                    r.left_top(),
                    egui::Align2::LEFT_TOP,
                    "Text",
                    egui::FontId::proportional(size * vp.zoom),
                    Color32::from_rgb(tr, tg, tb),
                );
            }
            _ => {}
        }
        painter.text(
            r.left_top() - Vec2::new(0.0, 4.0),
            egui::Align2::LEFT_BOTTOM,
            pl.purpose.hint(),
            egui::FontId::proportional(12.0),
            color,
        );
    }

    /// A rectangle in page coordinates, on screen.
    pub(super) fn page_rect_to_screen(
        &self,
        area: Rect,
        vp: &Viewport,
        page: usize,
        t: &TextRect,
    ) -> Rect {
        let p = self.view.layout.page(page);
        let (ax, ay) = vp.to_view(p.x + t.x0, p.y + t.y0);
        let (bx, by) = vp.to_view(p.x + t.x1, p.y + t.y1);
        Rect::from_min_max(area.min + Vec2::new(ax, ay), area.min + Vec2::new(bx, by))
    }

    pub(super) fn place_hover_cursor(&self, pos: Option<Pos2>) -> Option<egui::CursorIcon> {
        (self.mode == Mode::Place && pos.is_some()).then_some(egui::CursorIcon::Crosshair)
    }
}

/// The next font size up or down the list.
pub(super) fn step_size(size: f32, bigger: bool) -> f32 {
    if bigger {
        TEXT_SIZES.iter().copied().find(|s| *s > size + 0.01)
    } else {
        TEXT_SIZES.iter().rev().copied().find(|s| *s < size - 0.01)
    }
    .unwrap_or(size)
}

/// A sticky note icon (a page with lines) filling `r`.
pub(super) fn note_icon(painter: &egui::Painter, r: Rect, fill: Color32) {
    painter.rect_filled(r, 2.0, fill);
    painter.rect_stroke(
        r,
        2.0,
        Stroke::new(1.0, Color32::from_gray(40)),
        StrokeKind::Inside,
    );
    let stroke = Stroke::new((r.height() / 14.0).max(1.0), Color32::from_gray(60));
    for i in 1..4 {
        let y = r.top() + r.height() * i as f32 / 4.0;
        painter.line_segment(
            [
                Pos2::new(r.left() + r.width() * 0.2, y),
                Pos2::new(r.right() - r.width() * 0.2, y),
            ],
            stroke,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_size_steps() {
        assert_eq!(step_size(12.0, true), 14.0);
        assert_eq!(step_size(12.0, false), 11.0);
        assert_eq!(step_size(13.0, false), 12.0);
        assert_eq!(step_size(72.0, true), 72.0);
        assert_eq!(step_size(6.0, false), 6.0);
    }
}
