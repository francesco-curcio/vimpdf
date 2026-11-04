//! Redaction: marking text, search matches and areas, and applying the marks
//! to a new file.

use eframe::egui::{Stroke, StrokeKind};

use super::place::{Purpose, REDACT_RED};
use super::*;
use crate::annotate::RedactReport;
use crate::annotate::{AnnotKind, EditRequest, NewAnnot};
use crate::text::{Shape, TextRect};

impl VimpdfApp {
    /// `x` in visual mode.
    pub(super) fn redact_selection(&mut self) {
        let parts = self.selection_parts(Shape::Tight);
        if parts.is_empty() {
            return self.error("Nothing selected");
        }
        self.request_marks(parts);
        self.exit_visual();
    }

    /// Redaction marks in the document, applied or not by `:redact`.
    pub(super) fn redaction_marks(&self) -> usize {
        self.annots
            .list
            .iter()
            .filter(|a| a.kind == AnnotKind::Redact)
            .count()
    }

    /// After a save: marks are only annotations, so the text under them is
    /// still in the saved file. Say so, in red, until `:redact` is used.
    pub(super) fn warn_unapplied_redactions(&mut self, what: &str) {
        let marks = self.redaction_marks();
        if marks > 0 {
            self.error(format!(
                "{what}, but {}: text still in the file. :redact removes it",
                marks_not_applied(marks)
            ));
        }
    }

    /// Mark text or an area. The message says it's only a mark: `:redact`
    /// is what removes the content.
    pub(super) fn request_marks(&mut self, parts: Vec<(usize, Vec<TextRect>)>) {
        self.info(
            "Marked for redaction — nothing is removed until :redact writes the redacted copy",
        );
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
            "Marked {n} matches of \"{q}\" for redaction: :redact removes them (into a new file)"
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
    pub(super) fn redaction_done(&mut self, ctx: &egui::Context, report: RedactReport) {
        if !report.leftover.is_empty() {
            return self.error(format!(
                "Wrote {} but text is still present under {} mark(s): {}. Don't share it.",
                report.path.display(),
                report.leftover.len(),
                report.leftover.join("; ")
            ));
        }
        let summary = redaction_summary(&report);
        let warn = !report.metadata.is_empty();
        if self.annots.unsaved_other {
            // Opening the result would lose the other unsaved changes.
            self.info(format!(
                "{summary} Save (:w) or discard (:e!) your other changes, then :open it."
            ));
        } else {
            // Only redaction marks were unsaved: switch to the redacted file.
            self.annots.unsaved = false;
            self.open(ctx, report.path, false);
            self.info(summary.clone());
        }
        if warn {
            self.error(summary);
        }
    }
}

/// What `:redact` tells: first what still needs a look, as a long message
/// gets cut off in the status bar.
fn redaction_summary(r: &RedactReport) -> String {
    let mut parts = Vec::new();
    if !r.metadata.is_empty() {
        parts.push(format!(
            "Don't share it yet: the document's properties still quote the text ({}).",
            r.metadata.join(", ")
        ));
    }
    let name = r.path.file_name().unwrap_or(r.path.as_os_str());
    parts.push(format!(
        "Redacted {} mark(s) on {} page(s) into {}, verified: no text left under the marks.",
        r.marks,
        r.pages,
        name.display()
    ));
    if r.notes_removed > 0 {
        parts.push(format!("Removed {} quoting note(s).", r.notes_removed));
    }
    if r.outline_titles > 0 {
        parts.push(format!(
            "Replaced {} quoting outline title(s).",
            r.outline_titles
        ));
    }
    parts.join(" ")
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

/// "1 redaction mark not applied", "3 redaction marks not applied".
pub(super) fn marks_not_applied(n: usize) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{n} redaction mark{s} not applied")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_says_what_else_went_and_what_stayed() {
        let mut report = RedactReport {
            path: "/a/long/way/a.pdf".into(),
            marks: 2,
            pages: 1,
            leftover: Vec::new(),
            notes_removed: 0,
            outline_titles: 0,
            metadata: Vec::new(),
        };
        assert!(!redaction_summary(&report).contains("Removed"));
        report.notes_removed = 1;
        report.outline_titles = 2;
        report.metadata = vec!["Title".into()];
        let text = redaction_summary(&report);
        assert!(text.contains("Removed 1 quoting note"), "{text}");
        assert!(text.contains("Replaced 2 quoting outline"), "{text}");
        assert!(text.starts_with("Don't share it yet"), "{text}");
        assert!(text.contains("(Title)"), "{text}");
        assert!(text.contains(" into a.pdf,"), "{text}");
    }

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
