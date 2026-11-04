//! Tabs: one document each.
//!
//! The active tab's state lives in the app's own fields, so the rest of the
//! code works on "the document" without knowing about tabs. The other tabs
//! are parked here and swapped in when switched to. Their threads (render,
//! search, annotation editor) keep running; what they send waits in their
//! channels until the tab is active again. Tabs restored from the last
//! session open only when first shown, so a long session starts quickly and
//! costs no threads for what isn't read.

use super::*;
use crate::ui::tabbar::TabLabel;

/// Everything that belongs to one document.
pub(super) struct Tab {
    view: DocView,
    viewport: Option<Viewport>,
    jumps: JumpList,
    hints: Option<HintState>,
    selection: Option<crate::selection::Selection>,
    visual_pending: Option<bool>,
    drag: Option<Drag>,
    menu: Option<ContextMenu>,
    outline: OutlineState,
    persist: Persistence,
    annots: Annotations,
    placing: Option<Placing>,
    pending_scroll: Vec2,
    scrolling_up: bool,
}

impl Tab {
    fn new(view: DocView) -> Self {
        Self {
            view,
            viewport: None,
            jumps: JumpList::default(),
            hints: None,
            selection: None,
            visual_pending: None,
            drag: None,
            menu: None,
            outline: OutlineState::default(),
            persist: Persistence::default(),
            annots: Annotations::default(),
            placing: None,
            pending_scroll: Vec2::ZERO,
            scrolling_up: false,
        }
    }

    fn label(&self) -> TabLabel {
        TabLabel {
            name: tab_name(&self.view),
            unsaved: self.annots.unsaved,
            active: false,
        }
    }
}

fn tab_name(view: &DocView) -> String {
    if crate::welcome::is_welcome(&view.path) {
        "Start".into()
    } else {
        view.file_name()
    }
}

/// A place in the row of tabs.
pub(super) enum Slot {
    /// The active tab: its state is in the app's fields.
    Active,
    Parked(Box<Tab>),
    /// From the last session, not opened yet.
    Unopened(PathBuf),
}

pub(super) struct Tabs {
    /// One slot per tab, in order.
    slots: Vec<Slot>,
    active: usize,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            slots: vec![Slot::Active],
            active: 0,
        }
    }
}

impl Tabs {
    /// The parked tab in slot `i`, leaving the slot marked active.
    fn take_parked(&mut self, i: usize) -> Option<Box<Tab>> {
        if !matches!(self.slots[i], Slot::Parked(_)) {
            return None;
        }
        match std::mem::replace(&mut self.slots[i], Slot::Active) {
            Slot::Parked(tab) => Some(tab),
            _ => None,
        }
    }
}

/// The slots of a session's `tabs` whose tab `shown` takes the active tab.
fn restored_slots(tabs: Vec<PathBuf>, shown: usize) -> Vec<Slot> {
    tabs.into_iter()
        .enumerate()
        .map(|(i, path)| {
            if i == shown {
                Slot::Active
            } else {
                Slot::Unopened(path)
            }
        })
        .collect()
}

fn unopened_name(path: &std::path::Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

impl VimpdfApp {
    /// Exchange the active document's state with `tab`'s.
    fn swap_tab(&mut self, tab: &mut Tab) {
        use std::mem::swap;
        swap(&mut self.view, &mut tab.view);
        swap(&mut self.viewport, &mut tab.viewport);
        swap(&mut self.jumps, &mut tab.jumps);
        swap(&mut self.hints, &mut tab.hints);
        swap(&mut self.selection, &mut tab.selection);
        swap(&mut self.visual_pending, &mut tab.visual_pending);
        swap(&mut self.drag, &mut tab.drag);
        swap(&mut self.menu, &mut tab.menu);
        swap(&mut self.outline, &mut tab.outline);
        swap(&mut self.persist, &mut tab.persist);
        swap(&mut self.annots, &mut tab.annots);
        swap(&mut self.placing, &mut tab.placing);
        swap(&mut self.pending_scroll, &mut tab.pending_scroll);
        swap(&mut self.scrolling_up, &mut tab.scrolling_up);
    }

    /// Run `f` with each parked tab swapped in, then put things back.
    fn for_each_parked(&mut self, mut f: impl FnMut(&mut Self)) {
        for i in 0..self.tabs.slots.len() {
            let Some(mut tab) = self.tabs.take_parked(i) else {
                continue;
            };
            self.swap_tab(&mut tab);
            f(self);
            self.swap_tab(&mut tab);
            self.tabs.slots[i] = Slot::Parked(tab);
        }
    }

    pub(super) fn tab_labels(&self) -> Vec<TabLabel> {
        self.tabs
            .slots
            .iter()
            .map(|slot| match slot {
                Slot::Parked(tab) => tab.label(),
                Slot::Unopened(path) => TabLabel {
                    name: unopened_name(path),
                    unsaved: false,
                    active: false,
                },
                Slot::Active => TabLabel {
                    name: tab_name(&self.view),
                    unsaved: self.annots.unsaved,
                    active: true,
                },
            })
            .collect()
    }

    pub(super) fn tab_count(&self) -> usize {
        self.tabs.slots.len()
    }

    /// The state of tab `i`, opening it if it hasn't been yet. One that can't
    /// be opened any more goes away, with a message.
    fn load_tab(&mut self, ctx: &egui::Context, i: usize) -> Option<Box<Tab>> {
        let Slot::Unopened(path) = &self.tabs.slots[i] else {
            return self.tabs.take_parked(i);
        };
        match DocView::open(ctx, path, self.config.author_name()) {
            Ok(view) => {
                self.tabs.slots[i] = Slot::Active;
                Some(Box::new(Tab::new(view)))
            }
            Err(e) => {
                self.error(format!("{e:#}"));
                self.tabs.slots.remove(i);
                if i < self.tabs.active {
                    self.tabs.active -= 1;
                }
                self.save_session();
                None
            }
        }
    }

    /// Switch to tab `i` (0-based).
    pub(super) fn switch_tab(&mut self, ctx: &egui::Context, i: usize) {
        if i == self.tabs.active || i >= self.tab_count() {
            return;
        }
        self.save_state();
        let Some(mut tab) = self.load_tab(ctx, i) else {
            return;
        };
        self.swap_tab(&mut tab);
        self.tabs.slots[self.tabs.active] = Slot::Parked(tab);
        self.tabs.active = i;
        self.tab_switched(ctx);
    }

    /// `gt` / `gT`: `n` tabs on, wrapping around (negative goes back).
    pub(super) fn cycle_tab(&mut self, ctx: &egui::Context, n: i64) {
        let count = self.tab_count() as i64;
        if count < 2 {
            return self.info("Only one tab");
        }
        let i = (self.tabs.active as i64 + n).rem_euclid(count);
        self.switch_tab(ctx, i as usize);
    }

    /// `{n}gt`: tab `n`, counted from 1.
    pub(super) fn goto_tab(&mut self, ctx: &egui::Context, n: usize) {
        if n == 0 || n > self.tab_count() {
            return self.error(format!("No tab {n} (there are {})", self.tab_count()));
        }
        self.switch_tab(ctx, n - 1);
    }

    /// Remember the open tabs for the next launch (`ui.restore_tabs`). Only
    /// the main window does: detached windows come and go.
    pub(super) fn save_session(&self) {
        if !self.config.restore_tabs || !is_main_window() {
            return;
        }
        let mut session = crate::state::Session::default();
        for (i, slot) in self.tabs.slots.iter().enumerate() {
            let path = match slot {
                Slot::Active => &self.view.path,
                Slot::Parked(tab) => &tab.view.path,
                Slot::Unopened(path) => path,
            };
            if crate::welcome::is_welcome(path) {
                continue;
            }
            if i == self.tabs.active {
                session.active = session.tabs.len();
            }
            session.tabs.push(path.clone());
        }
        if let Some(dir) = crate::state::state_dir()
            && let Err(e) = crate::state::save_session(&dir, &session)
        {
            tracing::warn!("cannot save the open tabs: {e:#}");
        }
    }

    /// Put the last session's tabs in the row. On the start page the tab
    /// that was active replaces it; otherwise they go after the file vimpdf
    /// was launched with. Either way the rest open when first shown.
    pub(super) fn restore_session_tabs(&mut self, ctx: &egui::Context) {
        let Some((tabs, active)) = self.restored.take().filter(|(tabs, _)| !tabs.is_empty()) else {
            return;
        };
        if !self.on_start_page() {
            self.tabs.slots.extend(tabs.into_iter().map(Slot::Unopened));
            return self.save_session();
        }
        let shown = active
            .and_then(|a| tabs.iter().position(|t| *t == a))
            .unwrap_or(0);
        let path = tabs[shown].clone();
        self.tabs.slots = restored_slots(tabs, shown);
        self.tabs.active = shown;
        self.open(ctx, path, false);
        self.save_session();
        ctx.request_repaint();
    }

    /// Reset what doesn't carry over to another document.
    fn tab_switched(&mut self, ctx: &egui::Context) {
        self.keys.reset();
        self.link_preview.close();
        self.show_props = false;
        if !matches!(self.mode, Mode::Normal | Mode::Command | Mode::Menu) {
            self.mode = Mode::Normal;
        }
        ctx.send_viewport_cmd(ViewportCommand::Title(crate::welcome::display_name(
            &self.view.path,
        )));
        self.save_session();
        ctx.request_repaint();
    }

    fn tab_with_path(&self, path: &std::path::Path) -> Option<usize> {
        self.tabs.slots.iter().position(|slot| match slot {
            Slot::Active => self.view.path == path,
            Slot::Parked(tab) => tab.view.path == path,
            Slot::Unopened(p) => p == path,
        })
    }

    /// Open `path` in a new tab after the current one; switch to it if it's
    /// already open. The start page is replaced instead of kept.
    pub(super) fn open_tab(&mut self, ctx: &egui::Context, path: PathBuf) {
        let path = self.resolve(path);
        let Ok(path) = std::fs::canonicalize(&path) else {
            return self.error(format!("cannot open '{}'", path.display()));
        };
        if let Some(i) = self.tab_with_path(&path) {
            if i == self.tabs.active {
                return self.info(format!("{} is already open", self.view.file_name()));
            }
            return self.switch_tab(ctx, i);
        }
        if self.on_start_page() && !self.annots.unsaved {
            return self.open(ctx, path, false);
        }
        let view = match DocView::open(ctx, &path, self.config.author_name()) {
            Ok(v) => v,
            Err(e) => return self.error(format!("{e:#}")),
        };
        self.add_tab(ctx, view);
        self.watch_file(ctx);
        self.info(format!("Opened {}", self.view.file_name()));
    }

    /// `:tabnew` without a file: a tab with the start page.
    pub(super) fn new_start_tab(&mut self, ctx: &egui::Context) {
        let view = crate::welcome::write()
            .map_err(|e| format!("{e:#}"))
            .and_then(|p| {
                DocView::open(ctx, &p, self.config.author_name()).map_err(|e| format!("{e:#}"))
            });
        match view {
            Ok(view) => {
                if let Some(i) = self.tab_with_path(&view.path) {
                    return self.switch_tab(ctx, i);
                }
                self.add_tab(ctx, view);
            }
            Err(e) => self.error(e),
        }
    }

    fn add_tab(&mut self, ctx: &egui::Context, view: DocView) {
        let at = self.tabs.active + 1;
        let tab = Slot::Parked(Box::new(Tab::new(view)));
        self.tabs.slots.insert(at, tab);
        self.switch_tab(ctx, at);
    }

    /// Close the active tab (the window, if it's the last). Callers check
    /// for unsaved annotations.
    pub(super) fn close_tab(&mut self, ctx: &egui::Context) {
        if self.tab_count() == 1 {
            return ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        self.save_state();
        let closing = self.tabs.active;
        let next = if closing + 1 < self.tab_count() {
            closing + 1
        } else {
            closing - 1
        };
        let Some(mut tab) = self.load_tab(ctx, next) else {
            return self.close_tab(ctx); // that one is gone: look again
        };
        self.swap_tab(&mut tab);
        // `tab` now holds the closed document; dropping it stops its threads.
        drop(tab);
        self.tabs.slots.remove(closing);
        self.tabs.active = if next > closing { next - 1 } else { next };
        self.tab_switched(ctx);
    }

    /// Why the active tab can't move to another window, if it can't. Its
    /// reading position, zoom and marks go along (saved, then restored
    /// there); unsaved annotations would not.
    fn cannot_move(&self) -> Option<&'static str> {
        if crate::welcome::is_welcome(&self.view.path) {
            Some("The start page stays in this window")
        } else if self.annots.unsaved {
            Some("Unsaved annotations: :w saves them, then the tab can move")
        } else {
            None
        }
    }

    /// `:detach`: move the active tab to a new window (another vimpdf).
    pub(super) fn detach_tab(&mut self, ctx: &egui::Context) {
        if self.tab_count() == 1 {
            return self.info("Only one tab: it already has the window to itself");
        }
        if let Some(why) = self.cannot_move() {
            return self.error(why);
        }
        self.save_state();
        let spawned = own_executable().and_then(|exe| {
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("--new-window");
            if let Some(config) = &self.config_path {
                cmd.arg("--config").arg(config);
            }
            cmd.arg(&self.view.path)
                .stdin(std::process::Stdio::null())
                .spawn()
        });
        match spawned {
            Ok(mut child) => {
                // Reap it when it quits, so it doesn't linger as a zombie.
                std::thread::spawn(move || child.wait());
                self.close_tab(ctx);
            }
            Err(e) => self.error(format!("cannot open a new window: {e}")),
        }
    }

    /// `:attach`: move the active tab to the main window (the one files
    /// opened from outside go to). The last tab closes this window.
    pub(super) fn attach_tab(&mut self, ctx: &egui::Context) {
        if is_main_window() {
            return self.info("This is the main window");
        }
        if let Some(why) = self.cannot_move() {
            return self.error(why);
        }
        self.save_state();
        if !send_to_main_window(&self.view.path) {
            return self.error("No main window to move the tab to");
        }
        self.close_tab(ctx);
    }

    /// Index of a tab with unsaved annotations, the active one first.
    fn unsaved_tab(&self) -> Option<usize> {
        if self.annots.unsaved {
            return Some(self.tabs.active);
        }
        self.tabs
            .slots
            .iter()
            .position(|slot| matches!(slot, Slot::Parked(t) if t.annots.unsaved))
    }

    /// `:qa`: quit unless a tab has unsaved annotations (then show it).
    pub(super) fn quit_all(&mut self, ctx: &egui::Context) {
        if let Some(i) = self.unsaved_tab() {
            self.switch_tab(ctx, i);
            return self.error("Unsaved annotations in this tab: :w saves, :qa! discards all");
        }
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    /// `:qa!`: quit, discarding unsaved annotations in every tab.
    pub(super) fn force_quit_all(&mut self, ctx: &egui::Context) {
        self.annots.unsaved = false;
        self.for_each_parked(|app| app.annots.unsaved = false);
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    /// For the window's close button: a tab with unsaved annotations, which
    /// is then shown.
    pub(super) fn show_unsaved_tab(&mut self, ctx: &egui::Context) -> bool {
        match self.unsaved_tab() {
            Some(i) => {
                self.switch_tab(ctx, i);
                true
            }
            None => false,
        }
    }

    /// Remember every tab's reading position (on quit).
    pub(super) fn save_all_states(&mut self) {
        self.save_state();
        self.for_each_parked(|app| app.save_state());
    }
}

/// Whether this window receives the files opened from outside.
pub(super) fn is_main_window() -> bool {
    #[cfg(unix)]
    return crate::platform::instance::is_main();
    #[cfg(not(unix))]
    return true;
}

fn send_to_main_window(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    return crate::platform::instance::forward(path);
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// This program's file. After a reinstall while running, Linux names the
/// replaced file "<path> (deleted)": start the new one at `<path>` instead.
fn own_executable() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    if exe.exists() {
        return Ok(exe);
    }
    let name = exe.to_string_lossy();
    match name.strip_suffix(" (deleted)") {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(exe),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restored_session_opens_only_the_tab_shown() {
        let tabs = vec!["a.pdf".into(), "b.pdf".into(), "c.pdf".into()];
        let slots = restored_slots(tabs, 1);
        assert!(matches!(&slots[0], Slot::Unopened(p) if p.ends_with("a.pdf")));
        assert!(matches!(slots[1], Slot::Active));
        assert!(matches!(&slots[2], Slot::Unopened(p) if p.ends_with("c.pdf")));
    }
}
