//! Reading comfort: recolor, the outline panel, remembered positions and
//! reloading when the file changes.

use super::*;
use crate::render::Recolor;
use crate::state::{self, FileState};
use crate::watch::FileWatcher;

pub(super) fn file_stamp(path: &std::path::Path) -> Option<(u64, std::time::SystemTime)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// Wait this long after the last change on disk before reloading, so a
/// file still being written (LaTeX) is read once it's complete.
const RELOAD_DELAY: Duration = Duration::from_millis(300);
/// Save the reading position at most this often while reading (it is also
/// saved on quit and when opening another file).
const SAVE_INTERVAL: Duration = Duration::from_secs(10);

/// How much `<` and `>` change the outline's width.
const OUTLINE_STEP: f32 = 40.0;

/// State of the outline panel.
#[derive(Default)]
pub(super) struct OutlineState {
    pub selected: usize,
    /// Scroll the list to the selection on the next frame.
    pub follow: bool,
}

/// Auto-reload and position saving.
#[derive(Default)]
pub(super) struct Persistence {
    watcher: Option<FileWatcher>,
    /// Size and modification time of the file as we last read or wrote it:
    /// change events that don't change it (vimpdf's own saves, events macOS
    /// replays from before the file was opened) are ignored.
    stamp: Option<(u64, std::time::SystemTime)>,
    /// Another copy of this document that has saved annotations.
    pub annotated_copy: Option<(PathBuf, usize)>,
    reload_due: Option<Instant>,
    last_saved: Option<(Instant, FileState)>,
}

impl VimpdfApp {
    pub(super) fn recolor(&self) -> Option<Recolor> {
        let rgb = |c: Color| [c.0[0], c.0[1], c.0[2]];
        self.config.recolor.then(|| Recolor {
            dark: rgb(self.config.colors.recolor_dark),
            light: rgb(self.config.colors.recolor_light),
        })
    }

    pub(super) fn page_color(&self) -> Color32 {
        if self.config.recolor {
            color32(self.config.colors.recolor_dark)
        } else {
            color32(self.config.colors.page_background)
        }
    }

    // --- Outline ---------------------------------------------------------

    pub(super) fn toggle_outline(&mut self) {
        if self.mode == Mode::Outline {
            self.mode = Mode::Normal;
            return;
        }
        let current = self
            .viewport
            .as_ref()
            .map_or(0, |vp| vp.centre_page(&self.view.layout));
        self.outline.selected = self.outline_entry_for(current);
        self.outline.follow = true;
        self.keys.reset();
        self.mode = Mode::Outline;
    }

    /// The last outline entry at or before `page`.
    pub(super) fn outline_entry_for(&self, page: usize) -> usize {
        self.view
            .outline
            .iter()
            .rposition(|item| {
                matches!(item.target, Some(LinkTarget::Internal { page: p, .. }) if p <= page)
            })
            .unwrap_or(0)
    }

    pub(super) fn pin_outline(&mut self) {
        self.config.outline_pinned = !self.config.outline_pinned;
        self.outline.follow = true;
        self.info(if self.config.outline_pinned {
            "Outline kept open (Tab focuses it)"
        } else {
            "Outline unpinned"
        });
    }

    fn ui_state(&self) -> state::UiState {
        state::UiState {
            outline_pinned: self.config.outline_pinned,
            outline_width: self.config.outline_width,
        }
    }

    /// Apply the outline layout chosen in an earlier session over the
    /// config file's.
    pub(super) fn restore_ui_state(&mut self) {
        if let Some(ui) = state::state_dir().and_then(|d| state::load_ui(&d)) {
            self.config.outline_pinned = ui.outline_pinned;
            self.config.outline_width = ui.outline_width.max(OUTLINE_MIN_WIDTH);
        }
        self.ui_saved = self.ui_state();
    }

    /// Write the outline layout when it changes (once a resize drag ends).
    pub(super) fn remember_ui_state(&mut self, ctx: &egui::Context) {
        let ui = self.ui_state();
        if ui == self.ui_saved || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        self.ui_saved = ui;
        if let Some(dir) = state::state_dir()
            && let Err(e) = state::save_ui(&dir, ui)
        {
            tracing::warn!("cannot save the outline layout: {e:#}");
        }
    }

    /// A key in the outline panel.
    pub(super) fn outline_key(&mut self, tok: &str) -> Option<Command> {
        let len = self.view.outline.len();
        let sel = &mut self.outline.selected;
        let last = len.saturating_sub(1);
        match tok {
            "j" | "<Down>" | "<C-n>" => *sel = (*sel + 1).min(last),
            "k" | "<Up>" | "<C-p>" => *sel = sel.saturating_sub(1),
            "<C-d>" | "<PageDown>" => *sel = (*sel + 10).min(last),
            "<C-u>" | "<PageUp>" => *sel = sel.saturating_sub(10),
            "g" | "<Home>" => *sel = 0,
            "G" | "<End>" => *sel = last,
            "<CR>" | "l" | "o" | "<Space>" => {
                self.mode = Mode::Normal;
                let target = self.view.outline.get(*sel)?.target.clone();
                return target.map(Command::FollowLink);
            }
            "p" => return Some(Command::PinOutline),
            "<" => {
                self.config.outline_width =
                    (self.config.outline_width - OUTLINE_STEP).max(OUTLINE_MIN_WIDTH);
            }
            ">" => self.config.outline_width += OUTLINE_STEP,
            "<Esc>" | "<Tab>" | "q" | "h" => self.mode = Mode::Normal,
            _ => {}
        }
        self.outline.follow = true;
        None
    }

    /// The cursor to show over the pages: none once the mouse has been
    /// still for `mouse.hide_after` seconds, whatever its shape (arrow, text
    /// caret, hand), so it doesn't sit on the text. A held button counts as
    /// moving, so drags keep it.
    pub(super) fn hide_idle_pointer(
        &mut self,
        ctx: &egui::Context,
        area: Rect,
        cursor: egui::CursorIcon,
    ) -> egui::CursorIcon {
        let (moved, pos) = ctx.input(|i| {
            let moved = i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::PointerMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::MouseWheel { .. }
                )
            });
            (moved || i.pointer.any_down(), i.pointer.hover_pos())
        });
        if moved {
            self.pointer_active = Instant::now();
        }
        let after = self.config.hide_pointer_after;
        let over_pages = pos.is_some_and(|p| area.contains(p)) && !ctx.egui_wants_pointer_input();
        if after <= 0.0 || !over_pages || self.start.is_some() {
            return cursor;
        }
        let idle = self.pointer_active.elapsed();
        let limit = Duration::from_secs_f32(after);
        if idle >= limit {
            egui::CursorIcon::None
        } else {
            ctx.request_repaint_after(limit - idle);
            cursor
        }
    }

    // --- Remembered positions -------------------------------------------

    fn file_state(&self) -> Option<FileState> {
        let vp = self.viewport.as_ref()?;
        // Annotations saved in the file: the current list unless some are
        // unsaved, in which case keep what was recorded before.
        let annotations = if self.annots.unsaved {
            self.persist
                .last_saved
                .as_ref()
                .map_or(0, |(_, s)| s.annotations)
        } else {
            self.annots.list.len()
        };
        Some(FileState {
            position: self.position()?,
            zoom: vp.zoom,
            fit: vp.fit,
            marks: self.jumps.marks(),
            fingerprint: self.view.fingerprint.clone(),
            annotations,
        })
    }

    pub(super) fn save_state(&mut self) {
        if crate::welcome::is_welcome(&self.view.path) {
            return;
        }
        let (Some(dir), Some(state)) = (state::state_dir(), self.file_state()) else {
            return;
        };
        if self.persist.last_saved.as_ref().map(|(_, s)| s) == Some(&state) {
            return;
        }
        if let Err(e) = state::save(&dir, &self.view.path, &state) {
            tracing::warn!("cannot save reading position: {e:#}");
        }
        self.persist.last_saved = Some((Instant::now(), state));
    }

    /// Save now and then while reading, so a crash loses little.
    pub(super) fn save_state_periodically(&mut self) {
        let due = self
            .persist
            .last_saved
            .as_ref()
            .is_none_or(|(t, _)| t.elapsed() >= SAVE_INTERVAL);
        if due {
            self.save_state();
            if let Some((t, _)) = self.persist.last_saved.as_mut() {
                *t = Instant::now();
            }
        }
    }

    /// Restore the saved zoom, fit, position and marks of the open file.
    pub(super) fn restore_state(&mut self) {
        self.persist.annotated_copy = None;
        if crate::welcome::is_welcome(&self.view.path) {
            return;
        }
        let Some(dir) = state::state_dir() else {
            return;
        };
        let own = state::load(&dir, &self.view.path);
        let copies = match &self.view.fingerprint {
            Some(fp) => state::find_copies(&dir, fp, &self.view.path),
            None => Vec::new(),
        };
        // Another copy of the same paper with saved annotations (e.g. this is
        // a fresh download of a paper you annotated before).
        let own_annotations = own.as_ref().map_or(0, |s| s.annotations);
        // Prefer a permanent copy over a temporary one (which may vanish).
        if own_annotations == 0
            && let Some((path, s)) = copies
                .iter()
                .filter(|(_, s)| s.annotations > 0)
                .min_by_key(|(p, _)| crate::platform::is_temporary(p))
        {
            self.persist.annotated_copy = Some((path.clone(), s.annotations));
        }
        let from_copy = own.is_none() && !copies.is_empty();
        let Some(saved) = own.or_else(|| copies.first().map(|(_, s)| s.clone())) else {
            self.announce_annotated_copy(false);
            return;
        };
        let layout = &self.view.layout;
        if let Some(vp) = self.viewport.as_mut() {
            match saved.fit {
                FitMode::Width => vp.fit_width(layout, PAGE_MARGIN),
                FitMode::Page => vp.fit_page(layout, PAGE_MARGIN),
                FitMode::None => vp.zoom_to(saved.zoom, 0.0, 0.0),
            }
        }
        self.goto_position(saved.position);
        for (c, pos) in &saved.marks {
            self.jumps.set_mark(*c, *pos);
        }
        tracing::debug!(
            page = saved.position.page + 1,
            zoom = saved.zoom,
            from_copy,
            "restored reading position"
        );
        if from_copy {
            // A different file: don't treat its state as ours already saved.
            self.persist.last_saved = None;
        } else {
            self.persist.last_saved = Some((Instant::now(), saved));
        }
        self.announce_annotated_copy(from_copy);
    }

    fn announce_annotated_copy(&mut self, restored_from_copy: bool) {
        match (&self.persist.annotated_copy, restored_from_copy) {
            (Some((path, n)), _) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.info(format!(
                    "You annotated another copy of this paper ({name}, {n} annotations): :annotated opens it"
                ));
            }
            (None, true) => self.info("Restored your place from another copy of this paper"),
            (None, false) => {}
        }
    }

    /// `:annotated`.
    pub(super) fn open_annotated_copy(&mut self, ctx: &egui::Context) {
        match self.persist.annotated_copy.clone() {
            Some((path, _)) => {
                if !self.has_unsaved() {
                    self.open(ctx, path, true);
                }
            }
            None => self.info("No other annotated copy of this paper is known"),
        }
    }

    // --- Reloading ------------------------------------------------------

    /// Remember the file's current size and time (after opening or saving).
    pub(super) fn remember_file_stamp(&mut self) {
        self.persist.stamp = file_stamp(&self.view.path);
    }

    pub(super) fn watch_file(&mut self, ctx: &egui::Context) {
        self.persist.reload_due = None;
        self.remember_file_stamp();
        if !self.config.auto_reload {
            self.persist.watcher = None;
            return;
        }
        let wake = ctx.clone();
        self.persist.watcher =
            match FileWatcher::new(&self.view.path, move || wake.request_repaint()) {
                Ok(w) => Some(w),
                Err(e) => {
                    tracing::warn!("cannot watch {} for changes: {e}", self.view.path.display());
                    None
                }
            };
    }

    /// Reload once the file has stopped changing.
    pub(super) fn check_reload(&mut self, ctx: &egui::Context) {
        if self.config.auto_reload != self.persist.watcher.is_some() {
            self.watch_file(ctx); // `:set auto_reload` changed
        }
        if self
            .persist
            .watcher
            .as_ref()
            .is_some_and(FileWatcher::changed)
        {
            self.persist.reload_due = Some(Instant::now() + RELOAD_DELAY);
        }
        match self.persist.reload_due {
            Some(due) if Instant::now() >= due => {
                self.persist.reload_due = None;
                if file_stamp(&self.view.path) == self.persist.stamp {
                    // Not really changed (our own `:w`, or a replayed event).
                } else if self.annots.unsaved {
                    self.error(
                        "The file changed on disk, but you have unsaved annotations: \
                         :w overwrites it, :e! discards your changes and reloads",
                    );
                } else if self.view.path.exists() {
                    self.open(ctx, self.view.path.clone(), true);
                }
            }
            Some(due) => ctx.request_repaint_after(due - Instant::now()),
            None => {}
        }
    }
}
