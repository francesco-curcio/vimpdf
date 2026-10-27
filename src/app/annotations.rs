//! Highlights, comments and the annotations panel; saving annotations.

use eframe::egui::{Pos2, Stroke};

use super::*;
use crate::annotate::{AnnotInfo, AnnotKind, EditEvent, EditRequest, NewAnnot};
use crate::text::TextRect;
use crate::ui::comment::{CommentEditor, Outcome as CommentOutcome};

/// Within this long, a second attempt to close the window with unsaved
/// annotations really closes it.
const CLOSE_AGAIN: Duration = Duration::from_secs(5);

/// What a comment being written belongs to.
pub(super) enum CommentTarget {
    /// A new highlight of these parts, created when the comment is saved.
    New {
        parts: Vec<(usize, Vec<TextRect>)>,
        color: [u8; 3],
    },
    Existing {
        page: usize,
        xref: i32,
    },
}

/// Annotation state of the app.
#[derive(Default)]
pub(super) struct Annotations {
    pub list: Vec<AnnotInfo>,
    pub unsaved: bool,
    /// Unsaved changes other than redaction marks.
    pub unsaved_other: bool,
    last_letter: Option<char>,
    comment: Option<(CommentTarget, CommentEditor)>,
    /// Panel selection, and whether to scroll to it.
    selected: usize,
    follow: bool,
    /// `c` pressed in the panel: waiting for a colour letter.
    await_color: bool,
    quit_after_save: bool,
    close_attempt: Option<Instant>,
    /// `:saveas` in progress: switch to this file once it's written.
    switch_after_save: Option<PathBuf>,
    /// The temporary-copy hint was shown for this document.
    temp_hint_shown: bool,
}

impl VimpdfApp {
    fn letter_color(&self, letter: char) -> Option<[u8; 3]> {
        let c = self.config.highlight_color(letter)?;
        Some([c.0[0], c.0[1], c.0[2]])
    }

    /// The selection as rectangles per page.
    pub(super) fn selection_parts(&self) -> Vec<(usize, Vec<TextRect>)> {
        let Some(sel) = self.selection else {
            return Vec::new();
        };
        let text = self.view.doc_text();
        sel.page_spans(&text)
            .into_iter()
            .filter_map(|(page, a, b)| Some((page, self.view.text(page)?.rects((a, b)))))
            .filter(|(_, rects)| !rects.is_empty())
            .collect()
    }

    /// `a<letter>` in visual mode.
    pub(super) fn highlight(&mut self, letter: Option<char>) {
        let letter = letter
            .or(self.annots.last_letter)
            .unwrap_or(self.config.highlight_default);
        let Some(color) = self.letter_color(letter) else {
            let letters: String = self
                .config
                .highlight_colors
                .iter()
                .map(|(k, _)| *k)
                .collect();
            return self.error(format!(
                "No colour '{letter}' (colours: {letters}, or a for the last one)"
            ));
        };
        let parts = self.selection_parts();
        if parts.is_empty() {
            return self.error("Nothing selected");
        }
        self.annots.last_letter = Some(letter);
        self.view.editor.request(EditRequest::Create(NewAnnot {
            kind: AnnotKind::Highlight,
            parts,
            color,
            contents: String::new(),
        }));
        self.exit_visual();
    }

    /// `c` in visual mode: write a comment; the highlight is created when
    /// it's saved.
    pub(super) fn comment_on_selection(&mut self) {
        let parts = self.selection_parts();
        if parts.is_empty() {
            return self.error("Nothing selected");
        }
        let letter = self
            .annots
            .last_letter
            .unwrap_or(self.config.highlight_default);
        let color = self
            .letter_color(letter)
            .or_else(|| {
                self.config
                    .highlight_colors
                    .first()
                    .map(|(_, c)| [c.0[0], c.0[1], c.0[2]])
            })
            .unwrap_or([255, 224, 102]);
        self.exit_visual();
        self.annots.comment = Some((
            CommentTarget::New { parts, color },
            CommentEditor::new("New comment", String::new()),
        ));
        self.mode = Mode::Comment;
    }

    pub(super) fn edit_comment(&mut self, page: usize, xref: i32) {
        let Some(a) = self
            .annots
            .list
            .iter()
            .find(|a| a.page == page && a.xref == xref)
        else {
            return self.error("Annotation not found");
        };
        let editor = CommentEditor::new(format!("Comment (page {})", page + 1), a.contents.clone());
        self.annots.comment = Some((CommentTarget::Existing { page, xref }, editor));
        self.mode = Mode::Comment;
    }

    pub(super) fn recolor_annot(&mut self, page: usize, xref: i32, letter: char) {
        match self.letter_color(letter) {
            Some(color) => {
                self.annots.last_letter = Some(letter);
                self.view
                    .editor
                    .request(EditRequest::SetColor { page, xref, color });
            }
            None => self.error(format!("No colour '{letter}'")),
        }
    }

    pub(super) fn delete_annot(&mut self, page: usize, xref: i32) {
        self.view.editor.request(EditRequest::Delete { page, xref });
    }

    pub(super) fn undo(&mut self) {
        self.view.editor.request(EditRequest::Undo);
    }

    // --- Saving and quitting ----------------------------------------------

    fn resolve(&self, p: PathBuf) -> PathBuf {
        if p.is_relative() {
            self.view.path.parent().map(|d| d.join(&p)).unwrap_or(p)
        } else {
            p
        }
    }

    /// `:w`: refuses to save into a temporary copy (a browser may delete it).
    pub(super) fn write(&mut self, path: Option<PathBuf>) {
        if path.is_none() && self.annots.unsaved && crate::platform::is_temporary(&self.view.path) {
            return self.error(
                "This PDF is a temporary copy (e.g. opened from a browser) and may be deleted: \
                 :saveas <path> keeps it with your annotations, :w! saves here anyway",
            );
        }
        self.write_unchecked(path);
    }

    /// `:w!`.
    pub(super) fn write_unchecked(&mut self, path: Option<PathBuf>) {
        if path.is_none() && !self.annots.unsaved {
            return self.info("No changes to save");
        }
        let path = path.map(|p| {
            if p.is_relative() {
                self.view.path.parent().map(|d| d.join(&p)).unwrap_or(p)
            } else {
                p
            }
        });
        self.view.editor.request(EditRequest::Save { path });
    }

    /// `:saveas <path>`: write a copy with the annotations, then open it at
    /// the same place.
    pub(super) fn save_as(&mut self, path: PathBuf) {
        let path = self.resolve(path);
        if path.exists() {
            return self.error(format!(
                "{} already exists; choose another name",
                path.display()
            ));
        }
        self.annots.switch_after_save = Some(path.clone());
        self.view
            .editor
            .request(EditRequest::Save { path: Some(path) });
    }

    pub(super) fn write_quit(&mut self, ctx: &egui::Context) {
        if self.annots.unsaved {
            self.annots.quit_after_save = true;
            self.write(None);
        } else {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// `q` / `:q`: refuses while there are unsaved annotations.
    pub(super) fn quit(&mut self, ctx: &egui::Context) {
        if self.annots.unsaved {
            return self.error(if crate::platform::is_temporary(&self.view.path) {
                "Unsaved annotations in a temporary copy: :saveas <path> keeps them, :q! discards"
            } else {
                "Unsaved annotations: :w saves, :wq saves and quits, :q! discards"
            });
        }
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    pub(super) fn force_quit(&mut self, ctx: &egui::Context) {
        self.annots.unsaved = false;
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    /// The window's close button / ⌘Q: warn once about unsaved annotations.
    pub(super) fn guard_close(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || !self.annots.unsaved {
            return;
        }
        let again = self
            .annots
            .close_attempt
            .is_some_and(|t| t.elapsed() < CLOSE_AGAIN);
        if !again {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.annots.close_attempt = Some(Instant::now());
            self.error("Unsaved annotations: close again to discard them, or :w to save");
        }
    }

    /// Whether opening another file / reloading must be refused.
    pub(super) fn has_unsaved(&mut self) -> bool {
        if self.annots.unsaved {
            self.error("Unsaved annotations: :w to save them first (:e! discards them)");
        }
        self.annots.unsaved
    }

    // --- Events from the editor -------------------------------------------

    pub(super) fn receive_edits(&mut self, ctx: &egui::Context) {
        while let Some(event) = self.view.editor.try_recv() {
            match event {
                EditEvent::Loaded(list) => {
                    tracing::debug!(count = list.len(), "annotations loaded");
                    self.annots.list = list;
                }
                EditEvent::Changed {
                    annots,
                    pages,
                    working_copy,
                    unsaved,
                    unsaved_other,
                } => {
                    self.annots.unsaved_other = unsaved_other;
                    tracing::debug!(count = annots.len(), ?pages, unsaved, "annotations changed");
                    if unsaved
                        && !self.annots.temp_hint_shown
                        && crate::platform::is_temporary(&self.view.path)
                    {
                        self.annots.temp_hint_shown = true;
                        self.info(
                            "This is a temporary copy that may be deleted: :saveas <path> keeps it with your annotations",
                        );
                    }
                    self.annots.list = annots;
                    self.annots.unsaved = unsaved;
                    self.view.content_changed(working_copy, &pages);
                    let n = self.annots.list.len();
                    self.annots.selected = self.annots.selected.min(n.saturating_sub(1));
                }
                EditEvent::Saved { path, original } => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if original {
                        self.annots.unsaved = false;
                        self.annots.unsaved_other = false;
                        self.remember_file_stamp();
                        self.info(format!("Saved annotations to {name}"));
                        if self.annots.quit_after_save {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    } else if self.annots.switch_after_save.as_ref() == Some(&path) {
                        // `:saveas`: the copy has everything; continue there.
                        self.annots.switch_after_save = None;
                        // Still "unsaved" while switching, so the old file's
                        // state doesn't claim to hold these annotations.
                        self.open(ctx, path.clone(), true);
                        self.annots.unsaved = false;
                        self.annots.unsaved_other = false;
                        self.info(format!("Saved as {} — now editing it", path.display()));
                    } else {
                        self.info(format!("Saved a copy as {}", path.display()));
                    }
                }
                EditEvent::Redacted {
                    path,
                    marks,
                    pages,
                    leftover,
                } => self.redaction_done(ctx, path, marks, pages, leftover),
                EditEvent::Info(msg) => self.info(msg),
                EditEvent::Error(msg) => {
                    self.annots.quit_after_save = false;
                    self.error(msg);
                }
            }
        }
    }

    // --- Comment editor ---------------------------------------------------

    pub(super) fn show_comment_editor(&mut self, ctx: &egui::Context) {
        let font_size = self.config.font_size;
        let Some((_, editor)) = self.annots.comment.as_mut() else {
            if self.mode == Mode::Comment {
                self.mode = Mode::Normal;
            }
            return;
        };
        match editor.show(ctx, font_size) {
            CommentOutcome::Editing => {}
            CommentOutcome::Cancel => {
                self.annots.comment = None;
                self.mode = Mode::Normal;
            }
            CommentOutcome::Save(text) => {
                let (target, _) = self.annots.comment.take().expect("checked above");
                self.mode = Mode::Normal;
                let req = match target {
                    CommentTarget::New { parts, color } => EditRequest::Create(NewAnnot {
                        kind: AnnotKind::Highlight,
                        parts,
                        color,
                        contents: text,
                    }),
                    CommentTarget::Existing { page, xref } => EditRequest::SetContents {
                        page,
                        xref,
                        contents: text,
                    },
                };
                self.view.editor.request(req);
            }
        }
    }

    // --- Panel --------------------------------------------------------------

    pub(super) fn toggle_annotations(&mut self) {
        if self.mode == Mode::Annotations {
            self.mode = Mode::Normal;
            return;
        }
        // Start at the first annotation at or after the page being read.
        let current = self
            .viewport
            .as_ref()
            .map_or(0, |vp| vp.centre_page(&self.view.layout));
        let mut order = self.panel_order();
        order.retain(|i| self.annots.list[*i].page >= current);
        self.annots.selected = order
            .first()
            .and_then(|i| self.panel_order().iter().position(|j| j == i))
            .unwrap_or(0);
        self.annots.follow = true;
        self.annots.await_color = false;
        self.keys.reset();
        self.mode = Mode::Annotations;
    }

    /// Indices into the list, in document order.
    fn panel_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.annots.list.len()).collect();
        order.sort_by(|&a, &b| {
            let (a, b) = (&self.annots.list[a], &self.annots.list[b]);
            let ya = a.rects.first().map_or(0.0, |r| r.y0);
            let yb = b.rects.first().map_or(0.0, |r| r.y0);
            (a.page, ya)
                .partial_cmp(&(b.page, yb))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        order
    }

    fn selected_annot(&self) -> Option<&AnnotInfo> {
        let order = self.panel_order();
        order
            .get(self.annots.selected)
            .map(|&i| &self.annots.list[i])
    }

    /// A key in the annotations panel.
    pub(super) fn annotations_key(&mut self, tok: &str) -> Option<Command> {
        let len = self.annots.list.len();
        let last = len.saturating_sub(1);
        if self.annots.await_color {
            self.annots.await_color = false;
            let mut chars = tok.chars();
            if let (Some(c), None) = (chars.next(), chars.next())
                && let Some(a) = self.selected_annot()
            {
                return Some(Command::RecolorAnnot {
                    page: a.page,
                    xref: a.xref,
                    letter: c,
                });
            }
            return None;
        }
        let sel = &mut self.annots.selected;
        match tok {
            "j" | "<Down>" | "<C-n>" => *sel = (*sel + 1).min(last),
            "k" | "<Up>" | "<C-p>" => *sel = sel.saturating_sub(1),
            "<C-d>" | "<PageDown>" => *sel = (*sel + 10).min(last),
            "<C-u>" | "<PageUp>" => *sel = sel.saturating_sub(10),
            "g" | "<Home>" => *sel = 0,
            "G" | "<End>" => *sel = last,
            "<CR>" | "l" | "o" | "<Space>" => {
                let a = self.selected_annot()?.clone();
                self.mode = Mode::Normal;
                return Some(self.jump_to_annot(&a));
            }
            "e" => {
                let a = self.selected_annot()?;
                return Some(Command::EditComment {
                    page: a.page,
                    xref: a.xref,
                });
            }
            "d" | "x" => {
                let a = self.selected_annot()?;
                return Some(Command::DeleteAnnot {
                    page: a.page,
                    xref: a.xref,
                });
            }
            "c" => self.annots.await_color = true,
            "u" => return Some(Command::Undo),
            "<Esc>" | "q" | "<C-a>" | "h" => self.mode = Mode::Normal,
            _ => {}
        }
        self.annots.follow = true;
        None
    }

    fn jump_to_annot(&self, a: &AnnotInfo) -> Command {
        let r = a.rects.first();
        Command::FollowLink(LinkTarget::Internal {
            page: a.page,
            x: None,
            y: r.map(|r| r.y0),
        })
    }

    pub(super) fn show_annotations_panel(
        &mut self,
        ctx: &egui::Context,
        area: Rect,
    ) -> Option<Command> {
        let order = self.panel_order();
        let rows: Vec<crate::ui::annotations::Row> = order
            .iter()
            .map(|&i| {
                let a = &self.annots.list[i];
                crate::ui::annotations::Row {
                    annot: a,
                    snippet: self.snippet(a),
                }
            })
            .collect();
        let clicked = crate::ui::annotations::show(
            ctx,
            area,
            &rows,
            self.annots.selected,
            self.annots.follow,
            self.config.font_size,
        );
        self.annots.follow = false;
        let i = clicked?;
        self.annots.selected = i;
        let a = self.annots.list[order[i]].clone();
        Some(self.jump_to_annot(&a))
    }

    /// The text an annotation covers, if the page text is loaded.
    fn snippet(&self, a: &AnnotInfo) -> String {
        const MAX: usize = 120;
        let Some(text) = self.view.text(a.page) else {
            return String::new();
        };
        let inside = |c: &crate::text::TextChar| {
            let (cx, cy) = ((c.x0 + c.x1) / 2.0, (c.y0 + c.y1) / 2.0);
            a.rects
                .iter()
                .any(|r| cx >= r.x0 && cx <= r.x1 && cy >= r.y0 && cy <= r.y1)
        };
        let s: String = text
            .chars
            .iter()
            .filter(|c| inside(c))
            .map(|c| c.c)
            .collect();
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if s.chars().count() > MAX {
            s.chars().take(MAX).collect::<String>() + "…"
        } else {
            s
        }
    }

    // --- On the page ------------------------------------------------------

    /// The annotation under a point of the page area.
    pub(super) fn annot_at(&self, area: Rect, pos: Pos2) -> Option<&AnnotInfo> {
        let vp = self.viewport.as_ref()?;
        let rel = pos - area.min;
        let (x, y) = vp.to_doc(rel.x, rel.y);
        let page = self.view.layout.page_containing(x, y)?;
        let r = self.view.layout.page(page);
        let (px, py) = (x - r.x, y - r.y);
        self.annots.list.iter().rev().find(|a| {
            a.page == page
                && a.kind != AnnotKind::Other
                && a.rects
                    .iter()
                    .any(|t| px >= t.x0 && px <= t.x1 && py >= t.y0 && py <= t.y1)
        })
    }

    /// Small markers on annotations that carry a comment, and a tooltip with
    /// the comment when the pointer is over one.
    pub(super) fn draw_annotation_markers(
        &self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        area: Rect,
        vp: &Viewport,
    ) {
        let layout = &self.view.layout;
        let visible = layout.pages_in(vp.y, vp.y + vp.doc_height());
        for a in &self.annots.list {
            if a.contents.is_empty() || !visible.contains(&a.page) {
                continue;
            }
            let Some(first) = a.rects.first() else {
                continue;
            };
            let p = layout.page(a.page);
            let (x, y) = vp.to_view(p.x + first.x0, p.y + first.y0);
            let centre = area.min + Vec2::new(x, y);
            let [r, g, b] = a.color;
            painter.circle(
                centre,
                5.0,
                Color32::from_rgb(r, g, b),
                Stroke::new(1.5, Color32::from_gray(40)),
            );
        }
        if self.mode != Mode::Normal && self.mode != Mode::Visual {
            return;
        }
        let hover = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| area.contains(*p));
        if let Some(pos) = hover
            && let Some(a) = self.annot_at(area, pos)
            && !a.contents.is_empty()
        {
            egui::Area::new(egui::Id::unique("vimpdf-annot-tip-area"))
                .order(egui::Order::Tooltip)
                .fixed_pos(pos + Vec2::new(14.0, 14.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.label(&a.contents);
                    });
                });
        }
    }
}
