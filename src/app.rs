use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Rect, Vec2, ViewportCommand};

use crate::config::{Color, Config};
use crate::document::DocInfo;
use crate::input::command::{Command, parse_ex};
use crate::input::keymap::{KeyState, Keymap};
use crate::input::{key_tokens, mouse};
use crate::jumplist::{JumpList, Position};
use crate::links::{self, HintAction, LinkTarget, PageLink};
use crate::modes::Mode;
use crate::render::ScaleKey;
use crate::search::{MatchId, SearchEvent, SearchRequest};
use crate::text;
use crate::ui::cmdline::{CmdLine, Outcome, TypeAhead, type_ahead};
use crate::ui::toolbar::{self, Toolbar};
use crate::ui::{self, fonts, help, statusbar};
use crate::view::DocView;
use crate::viewport::{FitMode, Viewport};

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

pub struct PdfvApp {
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
    message: Option<Message>,
    /// Keyboard scroll distance not yet applied (smooth scrolling).
    pending_scroll: Vec2,
    /// Direction of the last vertical movement, for prefetching.
    scrolling_up: bool,
    first_frame: bool,
    jumps: JumpList,
    hints: Option<HintState>,
}

/// Link hints on screen (`f`): labels, and what has been typed so far.
struct HintState {
    action: HintAction,
    typed: String,
    hints: Vec<(String, usize, PageLink)>,
}

impl PdfvApp {
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

        let mut app = Self {
            view: DocView::with_info(ctx, path, info),
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
            message: None,
            pending_scroll: Vec2::ZERO,
            scrolling_up: false,
            first_frame: true,
            jumps: JumpList::default(),
            hints: None,
        };
        app.install_font(ctx, &mut warnings);
        app.report_warnings(warnings);
        app
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
        let view = match DocView::open(ctx, &path) {
            Ok(v) => v,
            Err(e) => return self.error(format!("{e:#}")),
        };

        let position = self.viewport.as_ref().map(|vp| {
            let layout = &self.view.layout;
            let page = vp.current_page(layout);
            (page, vp.y - layout.page(page).y)
        });
        self.view = view;
        self.pending_scroll = Vec2::ZERO;
        let name = self.view.file_name();
        ctx.send_viewport_cmd(ViewportCommand::Title(name.clone()));

        match (keep_position, position, self.viewport.as_mut()) {
            (true, Some((page, offset)), Some(vp)) => {
                let page = page.min(self.view.page_count() - 1);
                vp.y = self.view.layout.page(page).y + offset;
                self.info(format!("Reloaded {name}"));
            }
            _ => {
                self.viewport = None;
                self.jumps = JumpList::default();
                self.info(format!("Opened {name}"));
            }
        }
    }

    fn source_config(&mut self, ctx: &egui::Context) {
        let Some(path) = self.config_path.clone() else {
            return self.error("no config file location (HOME not set?)");
        };
        let (config, mut warnings) = Config::load(&path);
        let (keymap, key_warnings) = config.normal_keymap();
        warnings.extend(key_warnings);
        self.config = config;
        self.keymap = keymap;
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
        let step = self.config.scroll_step;
        let (screen, zoom_step) = match &self.viewport {
            Some(vp) => (vp.height, self.config.zoom_step),
            None => return,
        };
        match cmd {
            Command::ScrollDown(n) => return self.scroll(0.0, n as f32 * step),
            Command::ScrollUp(n) => return self.scroll(0.0, -(n as f32) * step),
            Command::ScrollLeft(n) => return self.scroll(-(n as f32) * step, 0.0),
            Command::ScrollRight(n) => return self.scroll(n as f32 * step, 0.0),
            Command::HalfPageDown(n) => return self.scroll(0.0, n as f32 * screen / 2.0),
            Command::HalfPageUp(n) => return self.scroll(0.0, -(n as f32) * screen / 2.0),
            Command::ScreenDown(n) => return self.scroll(0.0, n as f32 * screen),
            Command::ScreenUp(n) => return self.scroll(0.0, -(n as f32) * screen),
            _ => {}
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
            Command::Hints(action) => self.start_hints(action),
            Command::JumpBack(n) => {
                if let Some(cur) = self.position()
                    && let Some(pos) = self.jumps.back(cur, n as usize)
                {
                    self.goto_position(pos);
                } else {
                    self.info("At the oldest jump");
                }
            }
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
            Command::GotoMark(c) => match (self.jumps.mark(c), self.position()) {
                (Some(target), Some(cur)) => {
                    self.jumps.push(cur);
                    self.goto_position(target);
                }
                _ => self.error(format!("Mark '{c}' not set")),
            },
            Command::FollowLink(LinkTarget::External(uri)) => match links::open_external(&uri) {
                Ok(()) => self.info(format!("Opening {uri}")),
                Err(err) => self.error(err),
            },
            Command::EnterCommandLine(prefill) => {
                self.keys.reset();
                self.mode = Mode::Command;
                self.cmdline.open(&prefill);
            }
            Command::EnterSearch(prefill) => {
                self.keys.reset();
                self.mode = Mode::Search;
                self.searchline.open(&prefill);
            }
            Command::Search(query) => {
                let page = vp.current_page(layout);
                let origin_y = vp.y - layout.page(page).y;
                self.start_search(query, page, origin_y);
            }
            Command::SearchNext(n) => self.search_step(n as usize, true),
            Command::SearchPrev(n) => self.search_step(n as usize, false),
            Command::ToggleStatusbar => self.config.statusbar = !self.config.statusbar,
            Command::ToggleToolbar => self.config.toolbar = !self.config.toolbar,
            Command::ToggleHelp => self.show_help = !self.show_help,
            Command::ToggleFullscreen => {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!full));
            }
            Command::Reload => self.open(ctx, self.view.path.clone(), true),
            Command::Open(path) => self.open(ctx, path, false),
            Command::Set { option, value } => match self.config.set_from_str(&option, &value) {
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
            },
            Command::Source => self.source_config(ctx),
            Command::Cancel => {
                self.message = None;
                self.show_help = false;
                self.view.search.highlight = false;
            }
            Command::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
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

    /// Label every link on screen (`f`, `yf`).
    fn start_hints(&mut self, action: HintAction) {
        let Some(vp) = &self.viewport else { return };
        let layout = &self.view.layout;
        let (vx0, vy0) = (vp.x, vp.y);
        let (vx1, vy1) = (vp.x + vp.doc_width(), vp.y + vp.doc_height());
        let mut visible = Vec::new();
        for page in layout.pages_in(vy0, vy1) {
            let r = layout.page(page);
            for link in self.view.links.get(page).into_iter().flatten() {
                let (x0, y0) = (r.x + link.x0, r.y + link.y0);
                let (x1, y1) = (r.x + link.x1, r.y + link.y1);
                if x1 >= vx0 && x0 <= vx1 && y1 >= vy0 && y0 <= vy1 {
                    visible.push((page, link.clone()));
                }
            }
        }
        if visible.is_empty() {
            return self.info("No links on screen");
        }
        // Reading order: top to bottom, then left to right.
        visible.sort_by(|(pa, a), (pb, b)| {
            (pa, a.y0.round() as i32, a.x0.round() as i32).cmp(&(
                pb,
                b.y0.round() as i32,
                b.x0.round() as i32,
            ))
        });
        let labels = links::hint_labels(visible.len());
        for (label, (page, link)) in labels.iter().zip(&visible) {
            tracing::trace!(label, page = page + 1, target = ?link.target, "hint");
        }
        self.hints = Some(HintState {
            action,
            typed: String::new(),
            hints: labels
                .into_iter()
                .zip(visible)
                .map(|(l, (p, link))| (l, p, link))
                .collect(),
        });
        self.keys.reset();
        self.mode = Mode::Hints;
    }

    /// A key typed in hint mode. Returns a command once a label is complete.
    fn hint_key(&mut self, ctx: &egui::Context, tok: &str) -> Option<Command> {
        let state = self.hints.as_mut()?;
        match tok {
            "<Esc>" => {}
            "<BS>" => {
                state.typed.pop();
                return None;
            }
            t if t.chars().count() == 1 => {
                state.typed.push_str(&t.to_lowercase());
                let matching: Vec<&(String, usize, PageLink)> = state
                    .hints
                    .iter()
                    .filter(|(l, _, _)| l.starts_with(&state.typed))
                    .collect();
                match matching.as_slice() {
                    [] => {}
                    [(label, _, link)] if *label == state.typed => {
                        let (action, target) = (state.action, link.target.clone());
                        self.hints = None;
                        self.mode = Mode::Normal;
                        return match action {
                            HintAction::Follow => Some(Command::FollowLink(target)),
                            HintAction::CopyAddress => {
                                let text = match &target {
                                    LinkTarget::External(uri) => uri.clone(),
                                    LinkTarget::Internal { page, .. } => {
                                        format!("page {}", page + 1)
                                    }
                                };
                                ctx.copy_text(text.clone());
                                self.info(format!("Copied {text}"));
                                None
                            }
                        };
                    }
                    _ => return None, // keep typing
                }
            }
            _ => return None,
        }
        // Esc, or a label that matches nothing.
        self.hints = None;
        self.mode = Mode::Normal;
        None
    }

    fn draw_hints(&self, painter: &egui::Painter, area: Rect, vp: &Viewport) {
        let Some(state) = &self.hints else { return };
        let font = FontId::monospace(self.config.font_size.max(11.0) - 1.0);
        let bg = color32(self.config.colors.hint_background);
        let layout = &self.view.layout;
        for (label, page, link) in &state.hints {
            if !label.starts_with(&state.typed) {
                continue;
            }
            let r = layout.page(*page);
            let (x, y) = vp.to_view(r.x + link.x0, r.y + link.y0);
            let pos = area.min + Vec2::new(x, y);
            let galley = painter.layout_no_wrap(label.to_uppercase(), font.clone(), Color32::BLACK);
            let rect = Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(3.0, 1.0));
            painter.rect_filled(rect, 3.0, bg);
            painter.galley(pos, galley, Color32::BLACK);
        }
    }

    fn start_search(&mut self, query: String, page: usize, origin_y: f32) {
        let query = if query.trim().is_empty() {
            self.view.search.query.clone()
        } else {
            query
        };
        if query.trim().is_empty() {
            return self.error("No previous search");
        }
        let search = &mut self.view.search;
        search.start(query.clone(), page, origin_y);
        self.view.searcher.request(SearchRequest {
            id: search.id,
            case_sensitive: text::is_case_sensitive(&query),
            query,
            start_page: page,
            page_count: self.view.page_count(),
        });
    }

    fn search_step(&mut self, n: usize, forward: bool) {
        let search = &mut self.view.search;
        match search.step(n, forward) {
            Some(m) => self.show_match(m),
            None if search.running => self.info("Still searching…"),
            None if search.query.is_empty() => self.error("No previous search"),
            None => {
                let q = search.query.clone();
                self.error(format!("Pattern not found: {q}"));
            }
        }
    }

    /// Handle search results from the worker; jump to the first match.
    fn receive_search(&mut self) {
        while let Some(event) = self.view.searcher.try_recv() {
            let search = &mut self.view.search;
            match event {
                SearchEvent::Matches { id, page, matches } if id == search.id => {
                    if let Some(m) = search.add(page, matches) {
                        self.show_match(m);
                    }
                }
                SearchEvent::Done { id } if id == search.id => {
                    if let Some(m) = search.finish() {
                        self.show_match(m);
                    } else if search.total() == 0 {
                        let q = search.query.clone();
                        self.error(format!("Pattern not found: {q}"));
                    }
                }
                _ => {} // results of an older search
            }
        }
    }

    /// Scroll so a search match is visible, a third of the way down.
    fn show_match(&mut self, m: MatchId) {
        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        let Some(rects) = self.view.search.rects(m) else {
            return;
        };
        let Some(first) = rects.first() else { return };
        let page = self.view.layout.page(m.0);
        let (x0, y0) = (page.x + first.x0, page.y + first.y0);
        let (x1, y1) = (page.x + first.x1, page.y + first.y1);
        if y0 < vp.y || y1 > vp.y + vp.doc_height() {
            let layout = &self.view.layout;
            let cur = vp.current_page(layout);
            self.jumps.push(Position {
                page: cur,
                offset: vp.y - layout.page(cur).y,
                x: vp.x,
            });
            vp.y = y0 - vp.doc_height() / 3.0;
        }
        if x0 < vp.x || x1 > vp.x + vp.doc_width() {
            vp.x = (x0 + x1) / 2.0 - vp.doc_width() / 2.0;
        }
        self.pending_scroll = Vec2::ZERO;
    }

    fn draw_search_highlights(&self, painter: &egui::Painter, area: Rect, vp: &Viewport) {
        let search = &self.view.search;
        if !search.highlight || search.results.is_empty() {
            return;
        }
        let fill = color32(self.config.colors.search_highlight);
        let [r, g, b, _] = self.config.colors.hint_background.0;
        let current_fill = Color32::from_rgba_unmultiplied(r, g, b, 150);
        let layout = &self.view.layout;
        for page in layout.pages_in(vp.y, vp.y + vp.doc_height()) {
            let Some(matches) = search.results.get(&page) else {
                continue;
            };
            let p = layout.page(page);
            for (i, rects) in matches.iter().enumerate() {
                let current = search.current == Some((page, i));
                for t in rects {
                    let (ax, ay) = vp.to_view(p.x + t.x0, p.y + t.y0);
                    let (bx, by) = vp.to_view(p.x + t.x1, p.y + t.y1);
                    let rect = Rect::from_min_max(
                        area.min + Vec2::new(ax, ay),
                        area.min + Vec2::new(bx, by),
                    )
                    .expand(1.0);
                    painter.rect_filled(rect, 2.0, if current { current_fill } else { fill });
                }
            }
        }
    }

    fn search_status(&self) -> String {
        let s = &self.view.search;
        if s.query.is_empty() {
            return String::new();
        }
        match (s.position(), s.running) {
            (Some(i), true) => format!("[{i}/{}+ searching…]", s.total()),
            (Some(i), false) => format!("[{i}/{}]", s.total()),
            (None, true) => "[searching…]".into(),
            (None, false) => String::new(),
        }
    }

    /// Key presses → commands, according to the mode.
    fn keyboard_commands(&mut self, ctx: &egui::Context) -> Vec<Command> {
        // A focused text field (command line, toolbar page box) gets the keys.
        if self.mode == Mode::Hints {
            let events = ctx.input(|i| i.events.clone());
            return key_tokens(&events)
                .iter()
                .filter_map(|tok| self.hint_key(ctx, tok))
                .collect();
        }
        if self.mode != Mode::Normal || ctx.egui_wants_keyboard_input() {
            return Vec::new();
        }
        let events = ctx.input(|i| i.events.clone());
        let mut commands = Vec::new();
        let mut tokens = key_tokens(&events).into_iter();
        while let Some(tok) = tokens.next() {
            if self.show_help {
                if matches!(tok.as_str(), "<Esc>" | "?" | "q") {
                    self.show_help = false;
                }
                continue;
            }
            let Some(cmd) = self.keys.feed(&tok, &self.keymap) else {
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

impl eframe::App for PdfvApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.first_frame {
            // Launched from a terminal, the window doesn't always get focus.
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            self.first_frame = false;
        }

        // Split the window: toolbar, pages, status bar / command line.
        let font_size = self.config.font_size;
        let mut area = ui.max_rect();
        let toolbar_rect = self.config.toolbar.then(|| {
            let h = toolbar::height(font_size);
            let r = Rect::from_min_size(area.min, Vec2::new(area.width(), h));
            area.min.y += h;
            r
        });
        let bottom_rect = (self.config.statusbar || self.mode != Mode::Normal).then(|| {
            let h = ui::bar_height(font_size);
            area.max.y -= h;
            Rect::from_min_size(area.left_bottom(), Vec2::new(area.width(), h))
        });

        if self.viewport.is_none() {
            self.viewport = Some(self.new_viewport(area));
        }
        if let Some(vp) = self.viewport.as_mut()
            && (vp.width, vp.height) != (area.width(), area.height())
        {
            vp.resize(area.width(), area.height());
            vp.refit(&self.view.layout, PAGE_MARGIN);
        }

        // Input → commands.
        let mut commands = self.keyboard_commands(&ctx);
        let mut cursor = egui::CursorIcon::Default;
        if !ctx.egui_wants_pointer_input()
            && let Some(vp) = &self.viewport
        {
            let m = mouse::handle(
                &ctx,
                area,
                vp,
                &self.view.layout,
                &self.view.links,
                self.config.zoom_modifier,
            );
            commands.extend(m.commands);
            cursor = m.cursor;
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
        let scale = ScaleKey::from_scale(vp.zoom * ctx.pixels_per_point());
        self.view.schedule(&ctx, &vp, scale, self.scrolling_up);
        let painter = ui.painter().with_clip_rect(area);
        painter.rect_filled(area, 0.0, BACKGROUND);
        let page_color = color32(self.config.colors.page_background);
        self.view.draw(&painter, area, &vp, scale, page_color);
        self.draw_search_highlights(&painter, area, &vp);
        self.draw_hints(&painter, area, &vp);

        // Bars and overlays.
        if self
            .message
            .as_ref()
            .is_some_and(|m| m.since.elapsed() >= MESSAGE_DURATION)
        {
            self.message = None;
        }
        if let Some(m) = &self.message {
            ctx.request_repaint_after(MESSAGE_DURATION.saturating_sub(m.since.elapsed()));
        }

        let page = vp.centre_page(&self.view.layout) + 1;
        let pages = self.view.page_count();
        let mut ui_commands = Vec::new();
        if let Some(rect) = toolbar_rect {
            ui_commands.extend(self.toolbar.show(ui, rect, page, pages, font_size));
        }
        match (bottom_rect, self.mode) {
            (Some(rect), Mode::Command) => match self.cmdline.show(ui, rect, font_size) {
                Outcome::Editing => {}
                Outcome::Cancel => self.mode = Mode::Normal,
                Outcome::Submit(line) => {
                    self.mode = Mode::Normal;
                    match parse_ex(&line) {
                        Ok(Some(cmd)) => ui_commands.push(cmd),
                        Ok(None) => {}
                        Err(e) => self.error(e),
                    }
                }
            },
            (Some(rect), Mode::Search) => match self.searchline.show(ui, rect, font_size) {
                Outcome::Editing => {}
                Outcome::Cancel => self.mode = Mode::Normal,
                Outcome::Submit(line) => {
                    self.mode = Mode::Normal;
                    ui_commands.push(Command::Search(line));
                }
            },
            (Some(rect), Mode::Normal | Mode::Hints) => {
                let file = self.view.file_name();
                let status = statusbar::Status {
                    mode: self.mode.label(),
                    file: &file,
                    page,
                    pages,
                    zoom_percent: vp.zoom * 100.0,
                    pending: self.keys.pending_display(),
                    search: self.search_status(),
                    message: self.message.as_ref().map(|m| (m.text.as_str(), m.error)),
                    font_size,
                };
                ui_commands.extend(statusbar::show(ui, rect, &status));
            }
            (None, _) => {
                // No status bar: show messages over the page area.
                if let Some(m) = &self.message {
                    painter.text(
                        area.left_bottom() + Vec2::new(8.0, -8.0),
                        Align2::LEFT_BOTTOM,
                        &m.text,
                        FontId::monospace(font_size),
                        if m.error {
                            ui::ERROR
                        } else {
                            Color32::LIGHT_GRAY
                        },
                    );
                }
            }
        }
        if self.show_help {
            help::show(&ctx, &self.keymap, &mut self.show_help, font_size);
        }

        if !ui_commands.is_empty() {
            for cmd in ui_commands {
                self.execute(cmd, &ctx);
            }
            ctx.request_repaint();
        }
        ctx.set_cursor_icon(cursor);
    }
}
