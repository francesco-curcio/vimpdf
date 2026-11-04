use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Rect, Vec2, ViewportCommand};

use crate::config::{Color, Config, OUTLINE_MIN_WIDTH};
use crate::document::DocInfo;
use crate::input::command::{Command, parse_ex};
use crate::input::keymap::{KeyState, Keymap};
use crate::input::{key_tokens, mouse};
use crate::jumplist::{JumpList, Position};
use crate::links::{self, HintAction, LinkTarget};
use crate::modes::Mode;
use crate::render::ScaleKey;
use crate::search::{MatchId, SearchEvent, SearchRequest};
use crate::text;
use crate::ui::cmdline::{CmdLine, Outcome, TypeAhead, type_ahead};
use crate::ui::toolbar::{self, Toolbar};
use crate::ui::{self, fonts, help, statusbar};
use crate::view::DocView;
use crate::viewport::{FitMode, Viewport};

mod annotations;
mod confirm;
mod frame;
mod hints;
mod menu;
mod place;
mod preview;
mod reading;
mod redact;
mod searching;
mod start;
mod tabs;
mod visual;

use annotations::Annotations;
use hints::HintState;
use place::Placing;
use reading::{OutlineState, Persistence};
use visual::{ContextMenu, Drag};

const BACKGROUND: Color32 = Color32::from_gray(40);
/// Space around pages in the fit modes, in screen points.
const PAGE_MARGIN: f32 = 16.0;
const MESSAGE_DURATION: Duration = Duration::from_secs(4);
/// Time constant of smooth keyboard scrolling.
const SMOOTH_SCROLL_TAU: f32 = 0.05;
/// Screen points shown above a link destination.
const LINK_CONTEXT: f32 = 24.0;

struct Message {
    text: String,
    error: bool,
    since: Instant,
}

pub struct VimpdfApp {
    view: DocView,
    viewport: Option<Viewport>,
    config: Config,
    config_path: Option<PathBuf>,
    installed_font: Option<String>,
    keymap: Keymap,
    keys: KeyState,
    mode: Mode,
    cmdline: CmdLine,
    searchline: CmdLine,
    toolbar: Toolbar,
    show_help: bool,
    help_scroll: help::HelpScroll,
    message: Option<Message>,
    /// Keyboard scroll distance not yet applied (smooth scrolling).
    pending_scroll: Vec2,
    /// Direction of the last vertical movement, for prefetching.
    scrolling_up: bool,
    first_frame: bool,
    /// Present mode to switch to on the first frame (see `present`).
    present: Option<eframe::SurfaceConfig>,
    jumps: JumpList,
    hints: Option<HintState>,
    visual_keymap: Keymap,
    selection: Option<crate::selection::Selection>,
    /// `v` was pressed while page text was still loading (value: linewise).
    visual_pending: Option<bool>,
    /// Mode to return to after a `:` or `/` prompt (normal or visual).
    prompt_return: Mode,
    drag: Option<Drag>,
    menu: Option<ContextMenu>,
    outline: OutlineState,
    persist: Persistence,
    annots: Annotations,
    /// The rectangle being placed in redact-area mode.
    placing: Option<Placing>,
    menu_state: ui::menubar::MenuState,
    /// Mode to return to when a menu closes.
    menu_return: Mode,
    show_props: bool,
    settings: Option<ui::settings::SettingsWindow>,
    /// The tab to show once the last session's tabs are open.
    /// The last session's tabs and the active one, until the first frame.
    restored: Option<(Vec<PathBuf>, Option<PathBuf>)>,
    /// Commands waiting for a viewport.
    held_commands: Vec<Command>,
    /// An image to put on the clipboard (`Y`), sent at the end of the frame.
    clipboard_image: Option<egui::ColorImage>,
    link_preview: preview::LinkPreview,
    /// A yes/no question in the status bar.
    confirm: Option<confirm::Confirm>,
    /// When the mouse last moved, to hide an idle pointer.
    pointer_active: Instant,
    /// The outline layout as last written to `ui.toml`.
    ui_saved: crate::state::UiState,
    tabs: tabs::Tabs,
    /// The start page's recent files, while it shows.
    start: Option<start::StartScreen>,
}

impl VimpdfApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        path: PathBuf,
        info: DocInfo,
        config: Config,
        config_path: Option<PathBuf>,
        mut warnings: Vec<String>,
    ) -> Self {
        let ctx = &cc.egui_ctx;
        // Cmd +/- would change egui's UI scale; zoom is ours to handle.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);

        let (keymap, key_warnings) = config.normal_keymap();
        warnings.extend(key_warnings);
        let (visual_keymap, key_warnings) = config.visual_keymap();
        warnings.extend(key_warnings);

        let mut app = Self {
            view: DocView::with_info(ctx, path, info, config.author_name()),
            viewport: None,
            config,
            config_path,
            installed_font: None,
            keymap,
            keys: KeyState::default(),
            mode: Mode::Normal,
            cmdline: CmdLine::new(':'),
            searchline: CmdLine::new('/'),
            toolbar: Toolbar::default(),
            show_help: false,
            help_scroll: help::HelpScroll::default(),
            message: None,
            pending_scroll: Vec2::ZERO,
            scrolling_up: false,
            first_frame: true,
            present: None,
            jumps: JumpList::default(),
            hints: None,
            visual_keymap,
            selection: None,
            visual_pending: None,
            prompt_return: Mode::Normal,
            drag: None,
            menu: None,
            outline: OutlineState::default(),
            persist: Persistence::default(),
            annots: Annotations::default(),
            placing: None,
            menu_state: Default::default(),
            menu_return: Mode::Normal,
            pointer_active: Instant::now(),
            tabs: tabs::Tabs::default(),
            start: None,
            ui_saved: crate::state::UiState {
                outline_pinned: false,
                outline_width: 0.0,
            },
            show_props: false,
            settings: None,
            restored: None,
            held_commands: Vec::new(),
            clipboard_image: None,
            link_preview: Default::default(),
            confirm: None,
        };
        app.install_font(ctx, &mut warnings);
        app.restore_ui_state();
        app.watch_file(ctx);
        app.report_warnings(warnings);
        app
    }

    /// Switch to this present mode on the first frame.
    pub fn with_present(mut self, present: Option<eframe::SurfaceConfig>) -> Self {
        self.present = present;
        self
    }

    /// Open `tabs` (the last session's) once the window is up, then show
    /// `active`.
    pub fn with_restored_tabs(mut self, tabs: Vec<PathBuf>, active: Option<PathBuf>) -> Self {
        self.restored = Some((tabs, active));
        self
    }

    fn info(&mut self, text: impl Into<String>) {
        let text = text.into();
        tracing::debug!("{text}");
        self.message = Some(Message {
            text,
            error: false,
            since: Instant::now(),
        });
    }

    fn error(&mut self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!("{text}");
        self.message = Some(Message {
            text,
            error: true,
            since: Instant::now(),
        });
    }

    fn report_warnings(&mut self, warnings: Vec<String>) {
        for w in &warnings {
            tracing::warn!("{w}");
        }
        match warnings.as_slice() {
            [] => {}
            [one] => self.error(one.clone()),
            [first, rest @ ..] => {
                self.error(format!("{first} (+{} more, see log)", rest.len()));
            }
        }
    }

    fn install_font(&mut self, ctx: &egui::Context, warnings: &mut Vec<String>) {
        let family = self.config.font_family.clone();
        if self.installed_font.as_ref() == Some(&family) {
            return;
        }
        if let Err(e) = fonts::install(ctx, &family) {
            warnings.push(format!("config: ui.font_family: {e}"));
        }
        self.installed_font = Some(family);
    }

    fn new_viewport(&self, area: Rect) -> Viewport {
        let layout = &self.view.layout;
        let mut vp = Viewport::new(area.width(), area.height());
        match self.config.default_fit {
            FitMode::Width => vp.fit_width(layout, PAGE_MARGIN),
            FitMode::Page => vp.fit_page(layout, PAGE_MARGIN),
            FitMode::None => vp.zoom_to(1.0, 0.0, 0.0),
        }
        vp.y = 0.0;
        if vp.fit == FitMode::Page {
            vp.goto_page(layout, 0);
        }
        vp
    }

    /// Open a document. With `keep_position`, stay on the same page and
    /// offset (reload); otherwise start at the top with the default fit.
    fn open(&mut self, ctx: &egui::Context, path: PathBuf, keep_position: bool) {
        let path = if path.is_relative() {
            self.view
                .path
                .parent()
                .map(|dir| dir.join(&path))
                .unwrap_or(path)
        } else {
            path
        };
        let view = match DocView::open(ctx, &path, self.config.author_name()) {
            Ok(v) => v,
            Err(e) => return self.error(format!("{e:#}")),
        };
        let same_file = view.path == self.view.path;
        if !same_file {
            self.save_state();
        }

        let position = self.viewport.as_ref().map(|vp| {
            let layout = &self.view.layout;
            let page = vp.current_page(layout);
            (page, vp.y - layout.page(page).y)
        });
        self.view = view;
        self.link_preview.close();
        self.pending_scroll = Vec2::ZERO;
        self.selection = None;
        self.hints = None;
        if matches!(self.mode, Mode::Visual | Mode::Hints | Mode::Outline) {
            self.mode = Mode::Normal;
        }
        if same_file {
            self.remember_file_stamp();
        } else {
            self.watch_file(ctx);
        }
        let name = self.view.file_name();
        ctx.send_viewport_cmd(ViewportCommand::Title(crate::welcome::display_name(
            &self.view.path,
        )));

        match (keep_position, position, self.viewport.as_mut()) {
            (true, Some((page, offset)), Some(vp)) => {
                let page = page.min(self.view.page_count() - 1);
                vp.y = self.view.layout.page(page).y + offset;
                if same_file {
                    self.info(format!("Reloaded {name}"));
                } else {
                    self.info(format!("Opened {name} at the same place"));
                }
            }
            _ => {
                self.viewport = None;
                self.jumps = JumpList::default();
                self.info(format!("Opened {name}"));
            }
        }
    }

    /// The Settings window, while open. Changes apply as they are made.
    fn show_settings(&mut self, ctx: &egui::Context) -> Option<Command> {
        use ui::settings::Outcome;
        let window = self.settings.as_mut()?;
        let outcome = window.show(ctx, &mut self.config);
        if window.apply_keys(&mut self.config) {
            self.rebuild_keymaps();
        }
        let mut warnings = Vec::new();
        self.install_font(ctx, &mut warnings);
        self.report_warnings(warnings);
        let mut command = None;
        match outcome? {
            Outcome::Save { options, keys } => {
                let Some(path) = self.config_path.clone() else {
                    self.error("no config file location (HOME not set?)");
                    return None;
                };
                match crate::config::save_settings(&path, &options, keys.as_ref()) {
                    Ok(()) => self.info(format!("Settings saved to {}", path.display())),
                    Err(e) => {
                        // Keep the window open, with the changes, to retry.
                        self.error(e);
                        return None;
                    }
                }
            }
            Outcome::Cancel(before) => {
                self.config = *before;
                self.rebuild_keymaps();
                let mut warnings = Vec::new();
                self.install_font(ctx, &mut warnings);
                self.report_warnings(warnings);
            }
            Outcome::EditFile => command = Some(Command::EditConfig),
        }
        self.settings = None;
        command
    }

    /// The keymaps from the config's bindings (after the Settings window
    /// changed them).
    fn rebuild_keymaps(&mut self) {
        let (keymap, mut warnings) = self.config.normal_keymap();
        let (visual_keymap, more) = self.config.visual_keymap();
        warnings.extend(more);
        self.keymap = keymap;
        self.visual_keymap = visual_keymap;
        self.keys.reset();
        self.report_warnings(warnings);
    }

    fn source_config(&mut self, ctx: &egui::Context) {
        let Some(path) = self.config_path.clone() else {
            return self.error("no config file location (HOME not set?)");
        };
        let (config, mut warnings) = Config::load(&path);
        let (keymap, key_warnings) = config.normal_keymap();
        warnings.extend(key_warnings);
        let (visual_keymap, key_warnings) = config.visual_keymap();
        warnings.extend(key_warnings);
        self.config = config;
        self.restore_ui_state();
        self.keymap = keymap;
        self.visual_keymap = visual_keymap;
        self.keys.reset();
        self.install_font(ctx, &mut warnings);
        if warnings.is_empty() {
            self.info(format!("Sourced {}", path.display()));
        } else {
            self.report_warnings(warnings);
        }
    }

    fn scroll(&mut self, dx: f32, dy: f32) {
        if self.config.smooth_scroll {
            self.pending_scroll += Vec2::new(dx, dy);
        } else if let Some(vp) = self.viewport.as_mut() {
            vp.scroll_by(dx, dy);
        }
    }

    /// While the scrollbar is dragged: the page number beside the thumb.
    fn draw_scrollbar_page(&self, painter: &egui::Painter, thumb: Rect, vp: &Viewport) {
        let page = vp.current_page(&self.view.layout) + 1;
        let galley = painter.layout_no_wrap(
            format!("{page} / {}", self.view.page_count()),
            egui::FontId::monospace(self.config.font_size),
            ui::BAR_TEXT,
        );
        let size = galley.size() + Vec2::new(16.0, 8.0);
        let rect = Rect::from_min_size(
            egui::pos2(thumb.min.x - size.x - 6.0, thumb.center().y - size.y / 2.0),
            size,
        );
        painter.rect_filled(rect, 4.0, ui::BAR_BACKGROUND);
        painter.galley(rect.min + Vec2::new(8.0, 4.0), galley, ui::BAR_TEXT);
    }

    fn apply_smooth_scroll(&mut self, ctx: &egui::Context) {
        if self.pending_scroll == Vec2::ZERO {
            return;
        }
        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.05);
        let step = if self.pending_scroll.length() < 0.5 {
            self.pending_scroll
        } else {
            self.pending_scroll * (1.0 - (-dt / SMOOTH_SCROLL_TAU).exp())
        };
        let before = (vp.x, vp.y);
        vp.scroll_by(step.x, step.y);
        vp.clamp(&self.view.layout);
        self.pending_scroll -= step;
        // Stop at the document edges instead of pushing forever.
        if vp.x == before.0 {
            self.pending_scroll.x = 0.0;
        }
        if vp.y == before.1 {
            self.pending_scroll.y = 0.0;
        }
        ctx.request_repaint();
    }

    fn execute(&mut self, cmd: Command, ctx: &egui::Context) {
        tracing::trace!(?cmd, "command");
        if self.ask_for_missing_folder(&cmd) {
            return;
        }
        let step = self.config.scroll_step;
        let screen = match &self.viewport {
            Some(vp) => vp.height,
            None => {
                // Opening a document drops the viewport until the next frame
                // lays it out: what comes right after it waits for that.
                tracing::debug!(?cmd, "command held: no viewport yet");
                self.held_commands.push(cmd);
                ctx.request_repaint();
                return;
            }
        };
        if self.scroll_command(&cmd, step, screen) {
            return;
        }

        // Everything else moves instantly (or not at all).
        self.pending_scroll = Vec2::ZERO;
        let is_jump = matches!(
            cmd,
            Command::GotoFirst
                | Command::GotoLast
                | Command::GotoPage(_)
                | Command::FollowLink(LinkTarget::Internal { .. })
        );
        if is_jump && let Some(pos) = self.position() {
            self.jumps.push(pos);
        }
        match cmd {
            Command::ScrollBy { .. }
            | Command::NextPage(_)
            | Command::PrevPage(_)
            | Command::GotoFirst
            | Command::GotoLast
            | Command::GotoPage(_)
            | Command::ZoomIn(_)
            | Command::ZoomOut(_)
            | Command::ZoomReset
            | Command::ZoomTo(_)
            | Command::ZoomBy { .. }
            | Command::FitWidth
            | Command::FitPage
            | Command::FollowLink(LinkTarget::Internal { .. }) => self.move_view(cmd),
            Command::Hints(action) => self.start_hints(action),
            Command::JumpBack(n) => self.jump_back(n as usize),
            Command::JumpForward(n) => match self.jumps.forward(n as usize) {
                Some(pos) => self.goto_position(pos),
                None => self.info("At the newest jump"),
            },
            Command::AwaitMark { .. } => {} // handled by the key state
            Command::SetMark(c) => {
                if let Some(pos) = self.position() {
                    self.jumps.set_mark(c, pos);
                    self.info(format!("Mark '{c}' set"));
                }
            }
            Command::GotoMark(c) => self.goto_mark(c),
            Command::FollowLink(LinkTarget::External(uri)) => match links::open_external(&uri) {
                Ok(()) => self.info(format!("Opening {uri}")),
                Err(err) => self.error(err),
            },
            Command::EnterCommandLine(prefill) => {
                self.enter_prompt(Mode::Command);
                self.cmdline.open(&prefill);
            }
            Command::EnterSearch(prefill) => {
                self.enter_prompt(Mode::Search);
                self.searchline.open(&prefill);
            }
            Command::Search(query) => self.search_from_here(query),
            Command::SearchNext(n) => self.search_step(n as usize, true),
            Command::SearchPrev(n) => self.search_step(n as usize, false),
            Command::ToggleStatusbar => self.config.statusbar = !self.config.statusbar,
            Command::ToggleToolbar => self.config.toolbar = !self.config.toolbar,
            Command::ToggleHelp => self.show_help = !self.show_help,
            Command::ToggleRecolor => self.toggle_recolor(),
            Command::ToggleOutline => self.toggle_outline(),
            Command::PinOutline => self.pin_outline(),
            Command::ToggleFullscreen => {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!full));
            }
            Command::Reload => {
                if !self.has_unsaved() {
                    self.open(ctx, self.view.path.clone(), true);
                }
            }
            Command::Revert => {
                self.annots.unsaved = false;
                self.open(ctx, self.view.path.clone(), true);
            }
            Command::Open(path) => self.open_tab(ctx, path),
            Command::Edit(path) => {
                if !self.has_unsaved() {
                    self.open(ctx, path, false);
                }
            }
            Command::NextTab => self.cycle_tab(ctx, 1),
            Command::PrevTab(n) => self.cycle_tab(ctx, -i64::from(n)),
            Command::GotoTab(n) => self.goto_tab(ctx, n),
            Command::NewTab => self.new_start_tab(ctx),
            Command::DetachTab => self.detach_tab(ctx),
            Command::AttachTab => self.attach_tab(ctx),
            Command::QuitAll => self.quit_all(ctx),
            Command::ForceQuitAll => self.force_quit_all(ctx),
            Command::AwaitHighlight => {} // handled by the key state
            Command::Highlight(letter) => self.highlight(letter),
            Command::Comment => self.comment_on_selection(),
            Command::Undo => self.undo(),
            Command::ToggleAnnotations => self.toggle_annotations(),
            Command::EditComment { page, xref } => self.edit_comment(page, xref),
            Command::RecolorAnnot { page, xref, letter } => self.recolor_annot(page, xref, letter),
            Command::DeleteAnnot { page, xref } => self.delete_annot(page, xref),
            Command::Write(path) => self.write(path),
            Command::WriteQuit => self.write_quit(ctx),
            Command::WriteForce => self.write_unchecked(None),
            Command::WriteOver(path) => self.write_unchecked(Some(path)),
            Command::SaveAs(path) => self.save_as(path),
            Command::OpenAnnotatedCopy => self.open_annotated_copy(ctx),
            Command::ForceQuit => self.force_quit(ctx),
            Command::RedactSelection => self.redact_selection(),
            Command::RedactArea => self.start_redact_area(),
            Command::PlaceNote => self.start_placing_note(),
            Command::PlaceText => self.start_placing_text(),
            Command::AddNoteAt { page, x, y } => self.add_note_at(page, x, y),
            Command::AddTextAt { page, x, y } => self.add_text_at(page, x, y),
            Command::MoveAnnot { page, xref } => self.start_moving(page, xref),
            Command::AnnotFontSize { page, xref, bigger } => {
                self.annot_font_size(page, xref, bigger);
            }
            Command::RemoveComment { page, xref } => self.remove_comment(page, xref),
            Command::RedactMatches => self.redact_matches(),
            Command::ApplyRedactions(path) => self.apply_redactions(path),
            Command::OpenMenu => self.open_menu(),
            Command::ToggleMenubar => self.config.menubar = !self.config.menubar,
            Command::ToggleScrollbar => self.config.scrollbar = !self.config.scrollbar,
            Command::EditConfig => self.edit_config(),
            Command::CopyArea => self.start_copy_area(),
            Command::OpenSettings => self.open_settings(),
            Command::OpenKeyBindings => {
                self.open_settings();
                if let Some(settings) = self.settings.as_mut() {
                    settings.show_keys();
                }
            }
            Command::ToggleProperties => self.show_props = !self.show_props,
            Command::Set { option, value } => self.set_option(ctx, &option, &value),
            Command::Source => self.source_config(ctx),
            Command::Cancel => self.cancel(),
            Command::Visual { linewise } => self.visual(linewise),
            Command::Move(motion, n) => self.move_caret(motion, n as usize),
            Command::SwapEnds => self.swap_ends(),
            Command::Yank => self.yank(ctx),
            Command::SearchSelection => {
                if let Some(cmd) = self.search_selection() {
                    self.execute(cmd, ctx);
                }
            }
            Command::WordHints => self.start_word_hints(),
            Command::DeleteHints => self.start_delete_hints(),
            Command::CopyText(text) => {
                ctx.copy_text(text.clone());
                self.info(format!("Copied {text}"));
            }
            Command::Quit => self.quit(ctx),
            Command::ScrollDown(_)
            | Command::ScrollUp(_)
            | Command::ScrollLeft(_)
            | Command::ScrollRight(_)
            | Command::HalfPageDown(_)
            | Command::HalfPageUp(_)
            | Command::ScreenDown(_)
            | Command::ScreenUp(_) => unreachable!("handled above"),
        }
    }

    /// The commands that scroll smoothly. True if `cmd` was one.
    fn scroll_command(&mut self, cmd: &Command, step: f32, screen: f32) -> bool {
        let (dx, dy) = match *cmd {
            Command::ScrollDown(n) => (0.0, n as f32 * step),
            Command::ScrollUp(n) => (0.0, -(n as f32) * step),
            Command::ScrollLeft(n) => (-(n as f32) * step, 0.0),
            Command::ScrollRight(n) => (n as f32 * step, 0.0),
            Command::HalfPageDown(n) => (0.0, n as f32 * screen / 2.0),
            Command::HalfPageUp(n) => (0.0, -(n as f32) * screen / 2.0),
            Command::ScreenDown(n) => (0.0, n as f32 * screen),
            Command::ScreenUp(n) => (0.0, -(n as f32) * screen),
            _ => return false,
        };
        let by_pages = !matches!(
            cmd,
            Command::ScrollDown(_)
                | Command::ScrollUp(_)
                | Command::ScrollLeft(_)
                | Command::ScrollRight(_)
        );
        if self.mode == Mode::Visual && by_pages {
            self.caret_follow_scroll(dy);
        }
        self.scroll(dx, dy);
        true
    }

    /// The commands that move or zoom the view at once.
    fn move_view(&mut self, cmd: Command) {
        let zoom_step = self.config.zoom_step;
        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        let layout = &self.view.layout;
        let (cx, cy) = (vp.width / 2.0, vp.height / 2.0);
        match cmd {
            Command::ScrollBy { dx, dy } => vp.scroll_by(dx, dy),
            Command::NextPage(n) => vp.next_page(layout, n as usize),
            Command::PrevPage(n) => vp.prev_page(layout, n as usize),
            Command::GotoFirst => vp.goto_page(layout, 0),
            Command::GotoLast => vp.goto_end(layout),
            Command::GotoPage(0) => self.error("Pages start at 1"),
            Command::GotoPage(n) if n > layout.len() => {
                self.error(format!("No page {n} (last is {})", layout.len()));
            }
            Command::GotoPage(n) => vp.goto_page(layout, n - 1),
            Command::ZoomIn(n) => vp.zoom_by(zoom_step.powi(n as i32), cx, cy),
            Command::ZoomOut(n) => vp.zoom_by(zoom_step.powi(-(n as i32)), cx, cy),
            Command::ZoomReset => vp.zoom_to(1.0, cx, cy),
            Command::ZoomTo(percent) => vp.zoom_to(percent / 100.0, cx, cy),
            Command::ZoomBy { factor, x, y } => vp.zoom_by(factor, x, y),
            Command::FitWidth => vp.fit_width(layout, PAGE_MARGIN),
            Command::FitPage => vp.fit_page(layout, PAGE_MARGIN),
            Command::FollowLink(LinkTarget::Internal { page, x, y }) => {
                vp.goto_page(layout, page);
                let r = layout.page(page.min(layout.len() - 1));
                if let Some(y) = y {
                    // A little context above the destination (e.g. a heading).
                    vp.y = r.y + y - LINK_CONTEXT / vp.zoom;
                }
                if let Some(x) = x
                    && (r.x + x < vp.x || r.x + x > vp.x + vp.doc_width())
                {
                    vp.x = r.x + x - LINK_CONTEXT / vp.zoom;
                }
            }
            _ => {}
        }
    }

    fn jump_back(&mut self, n: usize) {
        if let Some(cur) = self.position()
            && let Some(pos) = self.jumps.back(cur, n)
        {
            self.goto_position(pos);
        } else {
            self.info("At the oldest jump");
        }
    }

    fn goto_mark(&mut self, c: char) {
        match (self.jumps.mark(c), self.position()) {
            (Some(target), Some(cur)) => {
                self.jumps.push(cur);
                self.goto_position(target);
            }
            _ => self.error(format!("Mark '{c}' not set")),
        }
    }

    /// Hand the keyboard to the command line or the search line.
    fn enter_prompt(&mut self, mode: Mode) {
        self.keys.reset();
        self.prompt_return = self.prompt_mode();
        self.mode = mode;
    }

    fn search_from_here(&mut self, query: String) {
        let Some(vp) = &self.viewport else { return };
        let layout = &self.view.layout;
        let page = vp.current_page(layout);
        let origin_y = vp.y - layout.page(page).y;
        self.start_search(query, page, origin_y);
    }

    fn toggle_recolor(&mut self) {
        self.config.recolor = !self.config.recolor;
        let state = if self.config.recolor { "on" } else { "off" };
        self.info(format!("Recolor {state}"));
    }

    fn start_placing_note(&mut self) {
        let color = self.note_color();
        self.start_placing(place::Purpose::Note { color });
    }

    fn start_placing_text(&mut self) {
        let color = self.text_color();
        let size = self.config.text_size;
        self.start_placing(place::Purpose::Text { size, color });
    }

    fn open_settings(&mut self) {
        if self.settings.is_none() {
            let file = self
                .config_path
                .as_deref()
                .map_or_else(Config::default, |p| Config::load(p).0);
            self.settings = Some(ui::settings::SettingsWindow::new(&self.config, file));
        }
    }

    fn set_option(&mut self, ctx: &egui::Context, option: &str, value: &str) {
        match self.config.set_from_str(option, value) {
            Ok(key) => {
                let mut warnings = Vec::new();
                self.install_font(ctx, &mut warnings);
                if warnings.is_empty() {
                    self.info(format!("{key} = {value}"));
                } else {
                    self.report_warnings(warnings);
                }
            }
            Err(e) => self.error(format!(":set {option}: {e}")),
        }
    }

    fn cancel(&mut self) {
        self.message = None;
        self.show_help = false;
        self.view.search.highlight = false;
        self.exit_visual();
    }

    fn position(&self) -> Option<Position> {
        let vp = self.viewport.as_ref()?;
        let layout = &self.view.layout;
        let page = vp.current_page(layout);
        Some(Position {
            page,
            offset: vp.y - layout.page(page).y,
            x: vp.x,
        })
    }

    fn goto_position(&mut self, pos: Position) {
        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        let layout = &self.view.layout;
        let page = pos.page.min(layout.len() - 1);
        vp.y = layout.page(page).y + pos.offset;
        vp.x = pos.x;
        self.pending_scroll = Vec2::ZERO;
    }

    /// Files opened from outside (Finder, Dock, browser), dropped on the
    /// window or restored from the last session each get a tab.
    fn open_external_files(&mut self, ctx: &egui::Context) {
        self.restore_session_tabs(ctx);
        if self.viewport.is_none() {
            return; // files wait for the next frame, as commands need one
        }
        let mut paths = crate::platform::take_opened();
        paths.extend(ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_owned())
                .filter(|p| !p.as_os_str().is_empty())
                .collect::<Vec<_>>()
        }));
        if paths.is_empty() {
            return;
        }
        let mut paths = paths.into_iter();
        for path in paths.by_ref() {
            self.execute(Command::Open(path), ctx);
            // Replacing the start page drops the viewport until the next
            // frame, and commands need one: the rest wait for it.
            if self.viewport.is_none() {
                break;
            }
        }
        crate::platform::push_opened(paths);
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        crate::platform::activate();
    }

    /// A command while the help overlay is open: scrolling commands scroll
    /// it, closing ones close it, everything else is ignored.
    fn help_key(&mut self, cmd: Command) {
        if matches!(cmd, Command::ToggleHelp | Command::Cancel | Command::Quit) {
            self.show_help = false;
            return;
        }
        let scroll = &mut self.help_scroll;
        scroll.key(&cmd, self.config.scroll_step);
        tracing::trace!(?cmd, offset = ?scroll.requested(), "help key");
    }

    /// The mode a prompt returns to: visual stays visual.
    fn prompt_mode(&self) -> Mode {
        if self.mode == Mode::Visual {
            Mode::Visual
        } else {
            Mode::Normal
        }
    }

    /// Key presses → commands, according to the mode.
    fn keyboard_commands(&mut self, ctx: &egui::Context) -> Vec<Command> {
        if tracing::enabled!(tracing::Level::TRACE) {
            let toks = key_tokens(&ctx.input(|i| i.events.clone()));
            if !toks.is_empty() {
                tracing::trace!(?toks, mode = ?self.mode, "keys");
            }
        }
        // A question in the status bar takes the next key.
        if self.confirm.is_some() {
            let events = ctx.input(|i| i.events.clone());
            if let Some(tok) = key_tokens(&events).into_iter().next() {
                self.keys.reset();
                self.confirm_key(ctx, &tok);
            }
            return Vec::new();
        }
        // The Settings window takes the keyboard: scrolling keys scroll it,
        // its own Esc closes it, text fields get what's typed in them.
        if let Some(settings) = self.settings.as_mut() {
            if !ctx.text_edit_focused() && !egui::Popup::is_any_open(ctx) {
                let events = ctx.input(|i| i.events.clone());
                for tok in key_tokens(&events) {
                    // The key bindings page takes keys being bound as they
                    // are, and its own keys unless a sequence is pending.
                    if (settings.recording() || !self.keys.is_pending()) && settings.key(&tok) {
                        self.keys.reset();
                        continue;
                    }
                    if let Some(cmd) = self.keys.feed(&tok, &self.keymap) {
                        settings.command(&cmd, self.config.scroll_step);
                    }
                }
            }
            return Vec::new();
        }
        // A focused text field (command line, toolbar page box) gets the keys.
        if self.mode == Mode::Hints {
            let events = ctx.input(|i| i.events.clone());
            return key_tokens(&events)
                .iter()
                .filter_map(|tok| self.hint_key(ctx, tok))
                .collect();
        }
        if self.mode == Mode::Outline {
            let events = ctx.input(|i| i.events.clone());
            return key_tokens(&events)
                .iter()
                .filter_map(|tok| self.outline_key(tok))
                .collect();
        }
        if self.mode == Mode::Menu {
            let events = ctx.input(|i| i.events.clone());
            return key_tokens(&events)
                .iter()
                .filter_map(|tok| self.menu_key(tok))
                .collect();
        }
        if self.mode == Mode::Place {
            let events = ctx.input(|i| i.events.clone());
            for tok in key_tokens(&events) {
                self.place_key(&tok);
            }
            return Vec::new();
        }
        if self.mode == Mode::Annotations {
            let events = ctx.input(|i| i.events.clone());
            return key_tokens(&events)
                .iter()
                .filter_map(|tok| self.annotations_key(tok))
                .collect();
        }
        if self.mode == Mode::LinkPreview {
            let events = ctx.input(|i| i.events.clone());
            return key_tokens(&events)
                .iter()
                .filter_map(|tok| self.preview_key(tok))
                .collect();
        }
        let visual = self.mode == Mode::Visual;
        if !(self.mode == Mode::Normal || visual) || ctx.text_edit_focused() {
            return Vec::new();
        }
        let events = ctx.input(|i| i.events.clone());
        let mut commands = Vec::new();
        let mut tokens = key_tokens(&events).into_iter();
        while let Some(tok) = tokens.next() {
            if self.show_props {
                if matches!(tok.as_str(), "<Esc>" | "q" | "<CR>") {
                    self.show_props = false;
                }
                continue;
            }
            if self.show_help {
                // Normal-mode bindings scroll the help instead of the page.
                if let Some(cmd) = self.keys.feed(&tok, &self.keymap) {
                    self.help_key(cmd);
                }
                continue;
            }
            if !visual
                && self.keys.pending_display().is_empty()
                && let Some(cmds) = self.start_key(&tok)
            {
                commands.extend(cmds);
                continue;
            }
            let keymap = if visual {
                &self.visual_keymap
            } else {
                &self.keymap
            };
            let Some(cmd) = self.keys.feed(&tok, keymap) else {
                continue;
            };
            let (prefill, search) = match cmd {
                Command::EnterCommandLine(p) => (p, false),
                Command::EnterSearch(p) => (p, true),
                Command::Hints(action) => {
                    // Label keys typed in the same frame as 'f' pick a hint.
                    self.start_hints(action);
                    while self.mode == Mode::Hints {
                        let Some(tok) = tokens.next() else { break };
                        commands.extend(self.hint_key(ctx, &tok));
                    }
                    continue;
                }
                other => {
                    commands.push(other);
                    continue;
                }
            };
            // Keys typed in the same frame as ':' or '/' belong to that line.
            match type_ahead(&mut tokens, prefill) {
                TypeAhead::Editing(text) if search => {
                    commands.push(Command::EnterSearch(text));
                    break;
                }
                TypeAhead::Editing(text) => {
                    commands.push(Command::EnterCommandLine(text));
                    break;
                }
                TypeAhead::Submitted(line) if search => {
                    self.searchline.push_history(&line);
                    commands.push(Command::Search(line));
                }
                TypeAhead::Submitted(line) => {
                    self.cmdline.push_history(&line);
                    match parse_ex(&line) {
                        Ok(Some(cmd)) => commands.push(cmd),
                        Ok(None) => {}
                        Err(e) => self.error(e),
                    }
                }
                TypeAhead::Cancelled => {}
            }
        }
        commands
    }
}

fn color32(c: Color) -> Color32 {
    let [r, g, b, a] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

impl eframe::App for VimpdfApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.start_frame(&ctx, frame);
        let font_size = self.config.font_size;
        let window = self.split_window(&ctx, ui.max_rect());
        let area = window.pages;

        if self.viewport.is_none() {
            self.viewport = Some(self.new_viewport(area));
            self.restore_state();
        }
        self.check_reload(&ctx);
        self.guard_close(&ctx);
        self.receive_edits(&ctx);
        self.open_external_files(&ctx);
        if let Some(vp) = self.viewport.as_mut()
            && (vp.width, vp.height) != (area.width(), area.height())
        {
            vp.resize(area.width(), area.height());
            vp.refit(&self.view.layout, PAGE_MARGIN);
        }

        // Input → commands.
        self.update_start_screen();
        let (commands, mut cursor) = self.frame_input(&ctx, area);
        if let Some(linewise) = self.visual_pending {
            self.visual(linewise);
        }
        let Some(vp) = &self.viewport else { return };
        let (y_before, zoom_before) = (vp.y, vp.zoom);
        for cmd in commands {
            self.execute(cmd, &ctx);
        }
        self.apply_smooth_scroll(&ctx);

        let Some(vp) = self.viewport.as_mut() else {
            // `:open` reset the viewport; lay out again next frame.
            ctx.request_repaint();
            return;
        };
        let mut scrollbar_drag = None;
        if let Some(rect) = window.scrollbar {
            let total = self.view.layout.height();
            let s = ui::scrollbar::show(ui, rect, vp.y, vp.doc_height(), total);
            if let Some(y) = s.y {
                vp.y = y;
                self.pending_scroll.y = 0.0;
            }
            scrollbar_drag = s.dragging.then_some(s.thumb);
        }
        vp.clamp(&self.view.layout);
        if vp.y != y_before {
            self.scrolling_up = vp.y < y_before;
            tracing::trace!(
                page = vp.centre_page(&self.view.layout) + 1,
                y = vp.y,
                "moved"
            );
        }
        let vp = vp.clone();
        if vp.zoom != zoom_before && !self.config.statusbar {
            self.info(format!("Zoom {:.0}%", vp.zoom * 100.0));
        }

        // Pages.
        self.receive_search();
        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        vp.clamp(&self.view.layout);
        let vp = vp.clone();
        self.view.receive_results(&ctx);
        self.view.request_text(&vp);
        self.draw_pages(ui, &ctx, area, &vp, scrollbar_drag);
        let (preview_command, preview_cursor) = self.show_link_preview(&ctx, area, &vp);
        if let Some(c) = preview_cursor {
            cursor = c;
        }
        let mut start_command = None;
        if self.start.is_some() {
            start_command = self.show_start_screen(ui, area);
        }

        // Bars and overlays.
        self.expire_message(&ctx);
        let mut ui_commands = Vec::new();
        ui_commands.extend(start_command);
        ui_commands.extend(preview_command);
        if let Some(rect) = window.tabbar {
            self.show_tabbar(ui, rect, &mut ui_commands);
        }
        if let Some(rect) = window.toolbar {
            let page = vp.centre_page(&self.view.layout) + 1;
            let pages = self.view.page_count();
            ui_commands.extend(self.toolbar.show(
                ui,
                rect,
                page,
                pages,
                vp.zoom * 100.0,
                font_size,
            ));
        }
        self.show_bottom_bar(ui, window.bottom, area, &vp, &mut ui_commands);
        if self.show_help {
            help::show(
                &ctx,
                &self.keymap,
                &self.visual_keymap,
                &mut self.show_help,
                &mut self.help_scroll,
                font_size,
            );
        }
        if let Some(cmd) = self.context_menu(&ctx) {
            ui_commands.push(cmd);
        }
        if self.show_props {
            self.show_properties(&ctx);
        }
        if let Some(cmd) = self.show_settings(&ctx) {
            ui_commands.push(cmd);
        }
        if let Some(rect) = window.menubar
            && let Some(cmd) = self.show_menubar(&ctx, ui, rect)
        {
            ui_commands.push(cmd);
        }
        if self.mode == Mode::Comment {
            self.show_comment_editor(&ctx);
        }
        if self.mode == Mode::Annotations
            && let Some(cmd) = self.show_annotations_panel(&ctx, area)
        {
            ui_commands.push(cmd);
        }
        if let Some(rect) = window.outline {
            self.show_outline_panel(&ctx, rect, &vp, &mut ui_commands);
        }
        self.save_state_periodically();
        self.remember_ui_state(&ctx);

        if !ui_commands.is_empty() {
            for cmd in ui_commands {
                self.execute(cmd, &ctx);
            }
            ctx.request_repaint();
        }
        ctx.set_cursor_icon(self.hide_idle_pointer(&ctx, area, cursor));
        if let Some(image) = self.clipboard_image.take() {
            ctx.copy_image(image);
        }
    }

    fn on_exit(&mut self) {
        self.save_all_states();
        self.save_session();
        crate::annotate::remove_own_work_dirs();
        #[cfg(unix)]
        crate::platform::instance::stop();
    }
}
