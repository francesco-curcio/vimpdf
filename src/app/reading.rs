//! Reading comfort: recolor, the outline panel, remembered positions and
//! reloading when the file changes.

use super::*;
use crate::render::Recolor;
use crate::state::{self, FileState};
use crate::watch::FileWatcher;

/// Wait this long after the last change on disk before reloading, so a
/// file still being written (LaTeX) is read once it's complete.
const RELOAD_DELAY: Duration = Duration::from_millis(300);
/// Save the reading position at most this often while reading (it is also
/// saved on quit and when opening another file).
const SAVE_INTERVAL: Duration = Duration::from_secs(10);

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
    reload_due: Option<Instant>,
    last_saved: Option<(Instant, FileState)>,
}

impl PdfvApp {
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
        // Select the last entry at or before the page being read.
        let current = self
            .viewport
            .as_ref()
            .map_or(0, |vp| vp.centre_page(&self.view.layout));
        self.outline.selected = self
            .view
            .outline
            .iter()
            .rposition(|item| {
                matches!(item.target, Some(LinkTarget::Internal { page, .. }) if page <= current)
            })
            .unwrap_or(0);
        self.outline.follow = true;
        self.keys.reset();
        self.mode = Mode::Outline;
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
            "<Esc>" | "<Tab>" | "q" | "h" => self.mode = Mode::Normal,
            _ => {}
        }
        self.outline.follow = true;
        None
    }

    // --- Remembered positions -------------------------------------------

    fn file_state(&self) -> Option<FileState> {
        let vp = self.viewport.as_ref()?;
        Some(FileState {
            position: self.position()?,
            zoom: vp.zoom,
            fit: vp.fit,
            marks: self.jumps.marks(),
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
        let Some(saved) = state::state_dir().and_then(|d| state::load(&d, &self.view.path)) else {
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
            "restored reading position"
        );
        self.persist.last_saved = Some((Instant::now(), saved));
    }

    // --- Reloading ------------------------------------------------------

    pub(super) fn watch_file(&mut self, ctx: &egui::Context) {
        self.persist.reload_due = None;
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
                if self.view.path.exists() {
                    self.open(ctx, self.view.path.clone(), true);
                }
            }
            Some(due) => ctx.request_repaint_after(due - Instant::now()),
            None => {}
        }
    }
}
