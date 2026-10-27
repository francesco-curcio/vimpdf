//! Visual mode, text selection with keyboard and mouse, context menu.

use eframe::egui::{CursorIcon, PointerButton, Pos2};

use super::*;
use crate::config::{DragMode, Modifier};
use crate::selection::{self, Motion, Selection, TextPos};

/// Rows of context kept between the caret and the window edge.
const CARET_MARGIN: f32 = 40.0;
/// Longest selection `*` will search for.
const MAX_SEARCH_SELECTION: usize = 200;

/// A mouse drag in progress.
#[derive(Debug, Clone, Copy)]
pub(super) enum Drag {
    /// Selecting text from this anchor (selection starts once it moves).
    Select { anchor: TextPos },
    /// Panning the view.
    Pan,
}

/// The right-click menu.
pub(super) struct ContextMenu {
    pos: Pos2,
    link: Option<LinkTarget>,
}

impl PdfvApp {
    /// `v` / `V`: start visual mode, switch between char- and line-wise, or
    /// leave it.
    pub(super) fn visual(&mut self, linewise: bool) {
        if self.mode == Mode::Visual
            && let Some(sel) = self.selection.as_mut()
        {
            if sel.linewise == linewise {
                self.exit_visual();
            } else {
                sel.linewise = linewise;
            }
            return;
        }
        match self.visual_start() {
            Ok(pos) => {
                let x = self.view.doc_text().rect_at(pos).map_or(0.0, |r| r.x0);
                let mut sel = Selection::at(pos, x);
                sel.linewise = linewise;
                self.selection = Some(sel);
                self.visual_pending = None;
                self.mode = Mode::Visual;
                self.ensure_caret_visible();
            }
            Err(true) => {
                // Text of a visible page is still loading: retry next frames.
                self.visual_pending = Some(linewise);
            }
            Err(false) => self.error("No text on screen (a scanned page?)"),
        }
    }

    /// Where `v` starts: the current search match if highlighted, else the
    /// first line in view. `Err(true)` while text is still loading.
    fn visual_start(&self) -> Result<TextPos, bool> {
        let search = &self.view.search;
        if search.highlight
            && let Some(m) = search.current.and_then(|c| search.get(c).map(|f| (c, f)))
            && self.view.text(m.0.0).is_some()
        {
            return Ok(TextPos {
                page: m.0.0,
                idx: m.1.span.0,
            });
        }
        let vp = self.viewport.as_ref().ok_or(false)?;
        let layout = &self.view.layout;
        let mut loading = false;
        for page in layout.pages_in(vp.y, vp.y + vp.doc_height()) {
            let Some(text) = self.view.text(page) else {
                loading = true;
                continue;
            };
            let top = vp.y - layout.page(page).y;
            let line = text.lines.iter().find(|l| {
                text.chars[(*l).clone()]
                    .iter()
                    .any(|c| c.y1 >= top && !c.c.is_whitespace())
            });
            if let Some(line) = line {
                return Ok(TextPos {
                    page,
                    idx: line.start,
                });
            }
        }
        Err(loading)
    }

    pub(super) fn exit_visual(&mut self) {
        self.selection = None;
        self.visual_pending = None;
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
    }

    pub(super) fn move_caret(&mut self, motion: Motion, n: usize) {
        let text = self.view.doc_text();
        let Some(sel) = self.selection.as_mut() else {
            return;
        };
        sel.caret = text.apply(sel.caret, motion, n, sel.want_x);
        if !matches!(motion, Motion::Up | Motion::Down)
            && let Some(r) = text.rect_at(sel.caret)
        {
            sel.want_x = (r.x0 + r.x1) / 2.0;
        }
        self.ensure_caret_visible();
    }

    /// In visual mode, scroll keys move the caret along with the view.
    pub(super) fn caret_follow_scroll(&mut self, dy_screen: f32) {
        let (Some(sel), Some(vp)) = (self.selection, self.viewport.as_ref()) else {
            return;
        };
        let text = self.view.doc_text();
        let Some(r) = text.rect_at(sel.caret) else {
            return;
        };
        let layout = &self.view.layout;
        let p = layout.page(sel.caret.page);
        let y = p.y + (r.y0 + r.y1) / 2.0 + dy_screen / vp.zoom;
        let page = layout.page_at_y(y.clamp(0.0, layout.height()));
        let target = self.view.text(page).and_then(|t| {
            let pr = layout.page(page);
            selection::hit_test(t, sel.want_x, y - pr.y)
        });
        if let (Some(idx), Some(s)) = (target, self.selection.as_mut()) {
            s.caret = TextPos { page, idx };
        }
    }

    /// Scroll just enough to keep the caret (plus some context) on screen.
    fn ensure_caret_visible(&mut self) {
        let Some(sel) = self.selection else { return };
        let Some(r) = self.view.doc_text().rect_at(sel.caret) else {
            return;
        };
        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        let p = self.view.layout.page(sel.caret.page);
        let margin = CARET_MARGIN / vp.zoom;
        let (y0, y1) = (p.y + r.y0, p.y + r.y1);
        if y0 - margin < vp.y {
            vp.y = y0 - margin;
        } else if y1 + margin > vp.y + vp.doc_height() {
            vp.y = y1 + margin - vp.doc_height();
        }
        let (x0, x1) = (p.x + r.x0, p.x + r.x1);
        if x0 < vp.x || x1 > vp.x + vp.doc_width() {
            vp.x = (x0 + x1) / 2.0 - vp.doc_width() / 2.0;
        }
        self.pending_scroll = Vec2::ZERO;
    }

    pub(super) fn swap_ends(&mut self) {
        if let Some(sel) = self.selection.as_mut() {
            std::mem::swap(&mut sel.anchor, &mut sel.caret);
            if let Some(r) = self.view.doc_text().rect_at(sel.caret) {
                sel.want_x = (r.x0 + r.x1) / 2.0;
            }
        }
        self.ensure_caret_visible();
    }

    fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        Some(sel.text(&self.view.doc_text())).filter(|t| !t.is_empty())
    }

    pub(super) fn yank(&mut self, ctx: &egui::Context) {
        match self.selected_text() {
            Some(text) => {
                let n = text.chars().count();
                ctx.copy_text(text);
                self.info(format!("Copied {n} characters"));
            }
            None => self.error("Nothing selected"),
        }
        self.exit_visual();
    }

    /// `*`: search for the selected text.
    pub(super) fn search_selection(&mut self) -> Option<Command> {
        let text = self.selected_text();
        self.exit_visual();
        match text {
            Some(t) if t.chars().count() <= MAX_SEARCH_SELECTION => {
                Some(Command::Search(text::normalize_query(&t)))
            }
            Some(_) => {
                self.error("Selection too long to search for");
                None
            }
            None => {
                self.error("Nothing selected");
                None
            }
        }
    }

    /// Put the caret on a search match (`/`, `n`, `N` in visual mode).
    pub(super) fn caret_to_match(&mut self, page: usize, idx: usize) {
        if self.mode == Mode::Visual
            && let Some(sel) = self.selection.as_mut()
        {
            sel.caret = TextPos { page, idx };
        }
    }

    pub(super) fn draw_selection(&self, painter: &egui::Painter, area: Rect, vp: &Viewport) {
        let Some(sel) = self.selection else { return };
        let text = self.view.doc_text();
        let layout = &self.view.layout;
        let fill = color32(self.config.colors.selection);
        let visible = layout.pages_in(vp.y, vp.y + vp.doc_height());
        let to_screen = |page: usize, t: &crate::text::TextRect| {
            let p = layout.page(page);
            let (ax, ay) = vp.to_view(p.x + t.x0, p.y + t.y0);
            let (bx, by) = vp.to_view(p.x + t.x1, p.y + t.y1);
            Rect::from_min_max(area.min + Vec2::new(ax, ay), area.min + Vec2::new(bx, by))
        };
        for (page, a, b) in sel.page_spans(&text) {
            if !visible.contains(&page) {
                continue;
            }
            let Some(pt) = self.view.text(page) else {
                continue;
            };
            for r in pt.rects((a, b)) {
                painter.rect_filled(to_screen(page, &r), 1.0, fill);
            }
        }
        if self.mode == Mode::Visual
            && let Some(r) = text.rect_at(sel.caret)
        {
            let rect = to_screen(sel.caret.page, &r);
            let caret = Rect::from_min_size(
                rect.min - Vec2::new(1.0, 0.0),
                Vec2::new(2.0, rect.height()),
            );
            painter.rect_filled(caret, 0.0, color32(self.config.colors.caret));
        }
    }

    /// The text position under a point in the page area.
    fn text_pos_at(&self, area: Rect, pos: Pos2) -> Option<TextPos> {
        let vp = self.viewport.as_ref()?;
        let rel = pos - area.min;
        let (x, y) = vp.to_doc(rel.x, rel.y);
        let layout = &self.view.layout;
        let page = layout.page_at_y(y.clamp(0.0, layout.height()));
        let r = layout.page(page);
        let idx = selection::hit_test(self.view.text(page)?, x - r.x, y - r.y)?;
        Some(TextPos { page, idx })
    }

    fn over_text(&self, area: Rect, pos: Pos2) -> bool {
        let Some(vp) = self.viewport.as_ref() else {
            return false;
        };
        let rel = pos - area.min;
        let (x, y) = vp.to_doc(rel.x, rel.y);
        let layout = &self.view.layout;
        let Some(page) = layout.page_containing(x, y) else {
            return false;
        };
        let r = layout.page(page);
        self.view
            .text(page)
            .is_some_and(|t| selection::over_text(t, x - r.x, y - r.y))
    }

    /// Mouse selection, panning and the context menu. Returns commands and,
    /// if the pointer is doing something text-related, the cursor to show.
    pub(super) fn mouse_selection(
        &mut self,
        ctx: &egui::Context,
        area: Rect,
    ) -> (Vec<Command>, Option<CursorIcon>) {
        let mut commands = Vec::new();
        let (pointer, modifiers) = ctx.input(|i| (i.pointer.clone(), i.modifiers));
        let hover = pointer.hover_pos().filter(|p| area.contains(*p));
        let pan_held = match self.config.pan_modifier {
            Modifier::Alt => modifiers.alt,
            Modifier::Shift => modifiers.shift,
            Modifier::Ctrl => modifiers.ctrl,
            Modifier::Cmd => modifiers.mac_cmd || modifiers.command,
        };

        // Start a drag.
        if let Some(press) = pointer.press_origin().filter(|p| area.contains(*p))
            && self.drag.is_none()
        {
            if pointer.button_pressed(PointerButton::Middle)
                || (pointer.primary_pressed() && (pan_held != (self.config.drag == DragMode::Pan)))
            {
                self.drag = Some(Drag::Pan);
            } else if pointer.primary_pressed()
                && !modifiers.shift
                && let Some(anchor) = self.text_pos_at(area, press)
            {
                self.drag = Some(Drag::Select { anchor });
            }
        }

        match self.drag {
            Some(Drag::Pan) => {
                let d = pointer.delta();
                if d != Vec2::ZERO {
                    commands.push(Command::ScrollBy { dx: -d.x, dy: -d.y });
                }
            }
            Some(Drag::Select { anchor }) if pointer.is_decidedly_dragging() => {
                if let Some(pos) = pointer.latest_pos()
                    && let Some(caret) = self.text_pos_at(area, pos)
                {
                    let x = self.view.doc_text().rect_at(caret).map_or(0.0, |r| r.x0);
                    self.selection = Some(Selection {
                        anchor,
                        caret,
                        linewise: false,
                        want_x: x,
                    });
                    self.mode = Mode::Visual;
                }
            }
            _ => {}
        }

        let released = !pointer.any_down();
        if released && let Some(drag) = self.drag.take() {
            let selected = matches!(drag, Drag::Select { .. }) && self.mode == Mode::Visual;
            if selected && self.config.copy_on_select {
                commands.push(Command::Yank);
            }
        }

        // Double / triple click select a word / line; Shift+click extends.
        let click_pos = pointer.interact_pos().filter(|p| area.contains(*p));
        if let Some(pos) = click_pos {
            let text = self.view.doc_text();
            if pointer.button_triple_clicked(PointerButton::Primary) {
                if let Some(p) = self.text_pos_at(area, pos)
                    && let Some((a, b)) = text.line_at(p)
                {
                    self.select_range(a, b);
                }
            } else if pointer.button_double_clicked(PointerButton::Primary) {
                if let Some(p) = self.text_pos_at(area, pos) {
                    let (a, b) = text.word_at(p);
                    self.select_range(a, b);
                }
            } else if pointer.primary_clicked()
                && modifiers.shift
                && let Some(p) = self.text_pos_at(area, pos)
            {
                match self.selection.as_mut() {
                    Some(sel) => sel.caret = p,
                    None => self.selection = Some(Selection::at(p, 0.0)),
                }
                self.mode = Mode::Visual;
            } else if pointer.primary_clicked() && self.mode == Mode::Visual {
                // A plain click clears the selection.
                self.exit_visual();
            }
        }

        if pointer.secondary_clicked()
            && let Some(pos) = click_pos
        {
            let link = self.viewport.as_ref().and_then(|vp| {
                mouse::link_at(
                    pos - area.min.to_vec2(),
                    vp,
                    &self.view.layout,
                    &self.view.links,
                )
                .map(|l| l.target.clone())
            });
            self.menu = Some(ContextMenu { pos, link });
        }

        let cursor = match self.drag {
            Some(Drag::Pan) => Some(CursorIcon::Grabbing),
            Some(Drag::Select { .. }) => Some(CursorIcon::Text),
            None if hover.is_some_and(|p| self.over_text(area, p)) => Some(CursorIcon::Text),
            None => None,
        };
        (commands, cursor)
    }

    fn select_range(&mut self, a: TextPos, b: TextPos) {
        let x = self.view.doc_text().rect_at(b).map_or(0.0, |r| r.x0);
        self.selection = Some(Selection {
            anchor: a,
            caret: b,
            linewise: false,
            want_x: x,
        });
        self.mode = Mode::Visual;
    }

    /// The right-click menu. Every entry names its key.
    pub(super) fn context_menu(&mut self, ctx: &egui::Context) -> Option<Command> {
        let menu = self.menu.as_ref()?;
        let has_selection = self.selection.is_some();
        let link = menu.link.clone();
        let mut chosen = None;
        let response = egui::Area::new(egui::Id::unique("pdfv-context-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(200.0);
                    let mut item =
                        |ui: &mut egui::Ui, label: &str, key: &str, on: bool, cmd: Command| {
                            let text = format!("{label}\t{key}");
                            if ui
                                .add_enabled(on, egui::Button::new(text).frame(false))
                                .clicked()
                            {
                                chosen = Some(cmd);
                            }
                        };
                    item(ui, "Copy", "y", has_selection, Command::Yank);
                    item(
                        ui,
                        "Search for selection",
                        "*",
                        has_selection,
                        Command::SearchSelection,
                    );
                    if let Some(target) = link {
                        ui.separator();
                        let address = match &target {
                            LinkTarget::External(uri) => uri.clone(),
                            LinkTarget::Internal { page, .. } => format!("page {}", page + 1),
                        };
                        item(ui, "Open link", "f", true, Command::FollowLink(target));
                        item(
                            ui,
                            "Copy link address",
                            "yf",
                            true,
                            Command::CopyText(address),
                        );
                    }
                });
            })
            .response;
        let clicked_elsewhere =
            ctx.input(|i| i.pointer.any_pressed()) && !response.contains_pointer();
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if chosen.is_some() || clicked_elsewhere || escape {
            self.menu = None;
        }
        chosen
    }
}
