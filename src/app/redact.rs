//! Redaction: marking text, search matches and areas, and applying the marks
//! to a new file.

use eframe::egui::{Pos2, Stroke, StrokeKind};

use super::*;
use crate::annotate::{AnnotKind, EditRequest, NewAnnot};
use crate::text::TextRect;

/// Movement of one `h`/`j`/`k`/`l` in area mode, in screen points.
const AREA_STEP: f32 = 10.0;
const REDACT_RED: Color32 = Color32::from_rgb(220, 40, 40);

/// The rectangle being placed in area mode, in page coordinates.
pub(super) struct AreaState {
    page: usize,
    rect: TextRect,
    /// Count typed before a key (`5l`).
    count: u32,
    /// Mouse drag start, in page coordinates.
    drag_from: Option<(f32, f32)>,
}

impl PdfvApp {
    /// `x` in visual mode.
    pub(super) fn redact_selection(&mut self) {
        let parts = self.selection_parts();
        if parts.is_empty() {
            return self.error("Nothing selected");
        }
        self.request_marks(parts);
        self.exit_visual();
    }

    fn request_marks(&mut self, parts: Vec<(usize, Vec<TextRect>)>) {
        self.view.editor.request(EditRequest::Create(NewAnnot {
            kind: AnnotKind::Redact,
            parts,
            color: [0, 0, 0],
            contents: String::new(),
        }));
    }

    /// `:redact-matches`: mark every match of the current search.
    pub(super) fn redact_matches(&mut self) {
        let search = &self.view.search;
        if search.query.is_empty() {
            return self.error("No search: search with / first, then :redact-matches");
        }
        if search.running {
            return self.error("Still searching: try again when the search is done");
        }
        let parts: Vec<(usize, Vec<TextRect>)> = search
            .results
            .iter()
            .map(|(&page, ms)| (page, ms.iter().flat_map(|m| m.rects.clone()).collect()))
            .filter(|(_, r): &(usize, Vec<TextRect>)| !r.is_empty())
            .collect();
        if parts.is_empty() {
            let q = search.query.clone();
            return self.error(format!("Pattern not found: {q}"));
        }
        let n: usize = search.results.values().map(Vec::len).sum();
        let q = search.query.clone();
        self.request_marks(parts);
        self.info(format!(
            "Marked {n} matches of \"{q}\" for redaction (:redact applies)"
        ));
    }

    /// `X`: place a rectangle on the page in the middle of the view.
    pub(super) fn start_redact_area(&mut self) {
        let Some(vp) = &self.viewport else { return };
        let layout = &self.view.layout;
        let page = vp.centre_page(layout);
        let r = layout.page(page);
        let (cx, cy) = vp.to_doc(vp.width / 2.0, vp.height / 2.0);
        let (w, h) = ((r.w / 3.0).min(200.0), (r.h / 8.0).min(100.0));
        let (x0, y0) = ((cx - r.x - w / 2.0).max(0.0), (cy - r.y - h / 2.0).max(0.0));
        self.area = Some(AreaState {
            page,
            rect: TextRect {
                x0,
                y0,
                x1: (x0 + w).min(r.w),
                y1: (y0 + h).min(r.h),
            },
            count: 0,
            drag_from: None,
        });
        self.keys.reset();
        self.mode = Mode::RedactArea;
        self.info(
            "Area: h j k l move, H J K L resize (counts work), Enter marks, Esc cancels; or drag",
        );
    }

    /// A key in area mode.
    pub(super) fn area_key(&mut self, tok: &str) {
        let zoom = self.viewport.as_ref().map_or(1.0, |vp| vp.zoom);
        let Some(area) = self.area.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        if let Some(d) = tok.chars().next().and_then(|c| c.to_digit(10))
            && tok.len() == 1
            && (d > 0 || area.count > 0)
        {
            area.count = (area.count * 10 + d).min(999);
            return;
        }
        let n = area.count.max(1) as f32 * AREA_STEP / zoom;
        area.count = 0;
        let page = self.view.layout.page(area.page);
        let r = &mut area.rect;
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
            "L" => r.x1 = (r.x1 + n).min(page.w),
            "H" => r.x1 = (r.x1 - n).max(r.x0 + min),
            "J" => r.y1 = (r.y1 + n).min(page.h),
            "K" => r.y1 = (r.y1 - n).max(r.y0 + min),
            "<CR>" => {
                let (page, rect) = (area.page, *r);
                self.area = None;
                self.mode = Mode::Normal;
                self.request_marks(vec![(page, vec![rect])]);
            }
            "<Esc>" | "q" => {
                self.area = None;
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    /// Dragging in area mode draws the rectangle.
    pub(super) fn area_mouse(&mut self, ctx: &egui::Context, area_rect: Rect) {
        let Some(vp) = self.viewport.clone() else {
            return;
        };
        let layout = &self.view.layout;
        let (pressed, down, pos) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
            )
        });
        let Some(pos) = pos.filter(|p| area_rect.contains(*p)) else {
            return;
        };
        let rel = pos - area_rect.min;
        let (x, y) = vp.to_doc(rel.x, rel.y);
        let Some(state) = self.area.as_mut() else {
            return;
        };
        if pressed && let Some(page) = layout.page_containing(x, y) {
            let r = layout.page(page);
            state.page = page;
            state.drag_from = Some((x - r.x, y - r.y));
        }
        if let Some((fx, fy)) = state.drag_from {
            let r = layout.page(state.page);
            let (px, py) = ((x - r.x).clamp(0.0, r.w), (y - r.y).clamp(0.0, r.h));
            if (px - fx).abs() > 2.0 && (py - fy).abs() > 2.0 {
                state.rect = TextRect {
                    x0: fx.min(px),
                    y0: fy.min(py),
                    x1: fx.max(px),
                    y1: fy.max(py),
                };
            }
            if !down {
                state.drag_from = None;
            }
        }
    }

    /// Redaction marks (red) and the area being placed.
    pub(super) fn draw_redactions(&self, painter: &egui::Painter, area: Rect, vp: &Viewport) {
        let layout = &self.view.layout;
        let visible = layout.pages_in(vp.y, vp.y + vp.doc_height());
        let to_screen = |page: usize, t: &TextRect| {
            let p = layout.page(page);
            let (ax, ay) = vp.to_view(p.x + t.x0, p.y + t.y0);
            let (bx, by) = vp.to_view(p.x + t.x1, p.y + t.y1);
            Rect::from_min_max(area.min + Vec2::new(ax, ay), area.min + Vec2::new(bx, by))
        };
        let fill = Color32::from_rgba_unmultiplied(220, 40, 40, 40);
        for a in &self.annots.list {
            if a.kind != AnnotKind::Redact || !visible.contains(&a.page) {
                continue;
            }
            for t in &a.rects {
                let r = to_screen(a.page, t);
                painter.rect_filled(r, 0.0, fill);
                painter.rect_stroke(r, 0.0, Stroke::new(1.5, REDACT_RED), StrokeKind::Outside);
            }
        }
        if let Some(state) = &self.area {
            let r = to_screen(state.page, &state.rect);
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(220, 40, 40, 70));
            painter.rect_stroke(r, 0.0, Stroke::new(2.0, REDACT_RED), StrokeKind::Outside);
            painter.text(
                r.left_top() - Vec2::new(0.0, 4.0),
                egui::Align2::LEFT_BOTTOM,
                "Enter marks · Esc cancels",
                egui::FontId::proportional(12.0),
                REDACT_RED,
            );
        }
    }

    /// `:redact [path]`.
    pub(super) fn apply_redactions(&mut self, path: Option<PathBuf>) {
        if !self.annots.list.iter().any(|a| a.kind == AnnotKind::Redact) {
            return self.error(
                "No redaction marks: x on a selection, X for an area, :redact-matches after a search",
            );
        }
        let out = match path {
            Some(p) if p.is_relative() => self.view.path.parent().map_or(p.clone(), |d| d.join(&p)),
            Some(p) => p,
            None => default_redacted_path(&self.view.path),
        };
        self.info(format!("Redacting into {}…", out.display()));
        self.view
            .editor
            .request(EditRequest::ApplyRedactions { out });
    }

    /// After the editor wrote the redacted file.
    pub(super) fn redaction_done(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        marks: usize,
        pages: usize,
        leftover: Vec<String>,
    ) {
        if !leftover.is_empty() {
            return self.error(format!(
                "Wrote {} but text is still present under {} mark(s): {}. Don't share it.",
                path.display(),
                leftover.len(),
                leftover.join("; ")
            ));
        }
        let summary = format!(
            "Redacted {marks} mark(s) on {pages} page(s) into {} — verified: no text left under the marks",
            path.display()
        );
        if self.annots.unsaved_other {
            // Opening the result would lose the other unsaved changes.
            self.info(format!(
                "{summary}. Save (:w) or discard (:e!) your other changes, then :open it."
            ));
        } else {
            // Only redaction marks were unsaved: switch to the redacted file.
            self.annots.unsaved = false;
            self.open(ctx, path, false);
            self.info(summary);
        }
    }

    pub(super) fn area_hover_cursor(&self, pos: Option<Pos2>) -> Option<egui::CursorIcon> {
        (self.mode == Mode::RedactArea && pos.is_some()).then_some(egui::CursorIcon::Crosshair)
    }
}

/// `paper.pdf` → `paper-redacted.pdf` (or `-redacted-2` … if it exists).
pub(super) fn default_redacted_path(path: &std::path::Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let mut candidate = dir.join(format!("{stem}-redacted.pdf"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}-redacted-{n}.pdf"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_path_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("pdfv-redpath-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("paper.pdf");
        assert_eq!(default_redacted_path(&src), dir.join("paper-redacted.pdf"));
        std::fs::write(dir.join("paper-redacted.pdf"), b"x").unwrap();
        assert_eq!(
            default_redacted_path(&src),
            dir.join("paper-redacted-2.pdf")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
