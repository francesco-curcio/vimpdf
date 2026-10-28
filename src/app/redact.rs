//! Redaction: marking text, search matches and areas, and applying the marks
//! to a new file.

use eframe::egui::{Stroke, StrokeKind};

use super::place::{Purpose, REDACT_RED};
use super::*;
use crate::annotate::{AnnotKind, EditRequest, NewAnnot};
use crate::text::TextRect;

impl VimpdfApp {
    /// `x` in visual mode.
    pub(super) fn redact_selection(&mut self) {
        let parts = self.selection_parts();
        if parts.is_empty() {
            return self.error("Nothing selected");
        }
        self.request_marks(parts);
        self.exit_visual();
    }

    pub(super) fn request_marks(&mut self, parts: Vec<(usize, Vec<TextRect>)>) {
        self.view.editor.request(EditRequest::Create(NewAnnot {
            kind: AnnotKind::Redact,
            parts,
            color: [0, 0, 0],
            contents: String::new(),
            font_size: 0.0,
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
        self.start_placing(Purpose::Redact);
    }

    /// Redaction marks (red).
    pub(super) fn draw_redactions(&self, painter: &egui::Painter, area: Rect, vp: &Viewport) {
        let visible = self.view.layout.pages_in(vp.y, vp.y + vp.doc_height());
        let fill = Color32::from_rgba_unmultiplied(220, 40, 40, 40);
        for a in &self.annots.list {
            if a.kind != AnnotKind::Redact || !visible.contains(&a.page) {
                continue;
            }
            for t in &a.rects {
                let r = self.page_rect_to_screen(area, vp, a.page, t);
                painter.rect_filled(r, 0.0, fill);
                painter.rect_stroke(r, 0.0, Stroke::new(1.5, REDACT_RED), StrokeKind::Outside);
            }
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
        let dir = std::env::temp_dir().join(format!("vimpdf-redpath-{}", std::process::id()));
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
