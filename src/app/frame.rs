//! One frame's work, in the order `ui` runs it: splitting the window, reading
//! input, drawing the pages, and the bars and panels around them.

use super::*;

/// The window, split into the bars and the pages.
pub(super) struct WindowRects {
    pub(super) menubar: Option<Rect>,
    pub(super) tabbar: Option<Rect>,
    pub(super) toolbar: Option<Rect>,
    pub(super) bottom: Option<Rect>,
    pub(super) outline: Option<Rect>,
    pub(super) scrollbar: Option<Rect>,
    pub(super) pages: Rect,
}

impl VimpdfApp {
    pub(super) fn start_frame(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(present) = self.present.take() {
            frame.set_wgpu_surface_config(present);
        }
        if self.first_frame {
            // Launched from a terminal, the window doesn't always get focus.
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            self.first_frame = false;
        }
        // vimpdf handles the keyboard itself: egui's Tab focus navigation must
        // not leave a button focused, or Enter/Space would "click" it. Only
        // text fields (command line, toolbar page box) keep focus.
        if !ctx.text_edit_focused()
            && let Some(id) = ctx.memory(|m| m.focused())
        {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
    }

    /// Split the window: toolbar, pages, status bar / command line.
    pub(super) fn split_window(&self, ctx: &egui::Context, mut area: Rect) -> WindowRects {
        let font_size = self.config.font_size;
        // Fullscreen is for reading: the bars above the pages step aside.
        // The menu bar still opens from the keyboard.
        let top_bars = !ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let menubar = ((self.config.menubar && top_bars) || self.mode == Mode::Menu).then(|| {
            let h = ui::bar_height(font_size);
            let r = Rect::from_min_size(area.min, Vec2::new(area.width(), h));
            area.min.y += h;
            r
        });
        let tabbar = (self.config.tabbar && top_bars).then(|| {
            let h = ui::tabbar::height(font_size);
            let r = Rect::from_min_size(area.min, Vec2::new(area.width(), h));
            area.min.y += h;
            r
        });
        let toolbar = (self.config.toolbar && top_bars).then(|| {
            let h = toolbar::height(font_size);
            let r = Rect::from_min_size(area.min, Vec2::new(area.width(), h));
            area.min.y += h;
            r
        });
        let bottom = (self.config.statusbar || self.mode != Mode::Normal || self.confirm.is_some())
            .then(|| {
                let h = ui::bar_height(font_size);
                area.max.y -= h;
                Rect::from_min_size(area.left_bottom(), Vec2::new(area.width(), h))
            });

        // The outline: pinned, it takes a strip on the left and the pages
        // move right; opened with <Tab> only, it covers the pages.
        let on_start = self.on_start_page();
        let outline = ((self.config.outline_pinned && !on_start) || self.mode == Mode::Outline)
            .then(|| {
                let max = (area.width() * 0.7).max(OUTLINE_MIN_WIDTH);
                let w = self.config.outline_width.clamp(OUTLINE_MIN_WIDTH, max);
                let r = Rect::from_min_size(area.min, Vec2::new(w, area.height()));
                if self.config.outline_pinned {
                    area.min.x += w;
                }
                r
            });
        // The scrollbar takes a strip on the right; the pages fit beside it.
        let scrollbar = (self.config.scrollbar && !on_start).then(|| {
            area.max.x -= ui::scrollbar::WIDTH;
            Rect::from_min_max(
                egui::pos2(area.max.x, area.min.y),
                egui::pos2(area.max.x + ui::scrollbar::WIDTH, area.max.y),
            )
        });
        WindowRects {
            menubar,
            tabbar,
            toolbar,
            bottom,
            outline,
            scrollbar,
            pages: area,
        }
    }

    /// This frame's commands from the keyboard and the pointer, and the
    /// cursor to show.
    pub(super) fn frame_input(
        &mut self,
        ctx: &egui::Context,
        area: Rect,
    ) -> (Vec<Command>, egui::CursorIcon) {
        let mut commands = Vec::new();
        if self.viewport.is_some() {
            commands.append(&mut self.held_commands);
        }
        commands.extend(self.keyboard_commands(ctx));
        let mut cursor = egui::CursorIcon::Default;
        if self.mode == Mode::Place && !ctx.egui_wants_pointer_input() {
            self.place_mouse(ctx, area);
            let hover = ctx
                .input(|i| i.pointer.hover_pos())
                .filter(|p| area.contains(*p));
            if let Some(c) = self.place_hover_cursor(hover) {
                cursor = c;
            }
        } else if !ctx.egui_wants_pointer_input() && self.menu.is_none() && self.start.is_none() {
            let (cmds, text_cursor) = self.mouse_selection(ctx, area);
            commands.extend(cmds);
            if let Some(c) = text_cursor {
                cursor = c;
            }
        }
        if !ctx.egui_wants_pointer_input()
            && self.menu.is_none()
            && self.start.is_none()
            && let Some(vp) = &self.viewport
        {
            let m = mouse::handle(
                ctx,
                area,
                vp,
                &self.view.layout,
                &self.view.links,
                self.config.zoom_modifier,
            );
            commands.extend(m.commands);
            if m.cursor != egui::CursorIcon::Default {
                cursor = m.cursor;
            }
        }
        (commands, cursor)
    }

    pub(super) fn draw_pages(
        &mut self,
        ui: &egui::Ui,
        ctx: &egui::Context,
        area: Rect,
        vp: &Viewport,
        scrollbar_drag: Option<Rect>,
    ) {
        let scale = ScaleKey::from_scale(vp.zoom * ctx.pixels_per_point());
        let recolor = self.recolor();
        self.view
            .schedule(ctx, vp, scale, recolor, self.scrolling_up);
        let painter = ui.painter().with_clip_rect(area);
        painter.rect_filled(area, 0.0, BACKGROUND);
        let page_color = self.page_color();
        self.view
            .draw(&painter, area, vp, scale, recolor, page_color);
        self.draw_search_highlights(&painter, area, vp);
        self.draw_annotation_markers(ctx, &painter, area, vp);
        self.draw_redactions(&painter, area, vp);
        self.draw_placing(&painter, area, vp);
        self.draw_selection(&painter, area, vp);
        self.draw_hints(&painter, area, vp);
        if let Some(thumb) = scrollbar_drag {
            self.draw_scrollbar_page(&painter, thumb, vp);
        }
    }

    pub(super) fn expire_message(&mut self, ctx: &egui::Context) {
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
    }

    pub(super) fn show_tabbar(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        commands: &mut Vec<Command>,
    ) {
        let font_size = self.config.font_size;
        let main = tabs::is_main_window();
        use ui::tabbar::TabAction;
        let action = ui::tabbar::show(ui, rect, &self.tab_labels(), font_size, main);
        let (tab, then) = match action {
            None => return,
            Some(TabAction::Switch(i)) => (i, None),
            Some(TabAction::Close(i)) => {
                // Nothing to open just to close it.
                if self.forget_unopened_tab(i) {
                    return;
                }
                (i, Some(Command::Quit))
            }
            Some(TabAction::Detach(i)) => (i, Some(Command::DetachTab)),
            Some(TabAction::Attach(i)) => (i, Some(Command::AttachTab)),
        };
        // Without this, what comes after would hit the tab already showing.
        if self.forget_missing_tab(tab) {
            return;
        }
        commands.push(Command::GotoTab(tab + 1));
        commands.extend(then);
    }

    /// The command line, the search line or the status bar, or without any
    /// of them the message over the pages.
    pub(super) fn show_bottom_bar(
        &mut self,
        ui: &mut egui::Ui,
        rect: Option<Rect>,
        area: Rect,
        vp: &Viewport,
        commands: &mut Vec<Command>,
    ) {
        let font_size = self.config.font_size;
        let page = vp.centre_page(&self.view.layout) + 1;
        let pages = self.view.page_count();
        match (rect, self.mode) {
            (Some(rect), Mode::Command) => match self.cmdline.show(
                ui,
                rect,
                font_size,
                Some(&|line: &str| {
                    let base = self.view.path.parent().unwrap_or(std::path::Path::new("/"));
                    crate::input::complete::complete(line, base)
                }),
            ) {
                Outcome::Editing => {}
                Outcome::Cancel => self.mode = self.prompt_return,
                Outcome::Submit(line) => {
                    self.mode = self.prompt_return;
                    match parse_ex(&line) {
                        Ok(Some(cmd)) => commands.push(cmd),
                        Ok(None) => {}
                        Err(e) => self.error(e),
                    }
                }
            },
            (Some(rect), Mode::Search) => match self.searchline.show(ui, rect, font_size, None) {
                Outcome::Editing => {}
                Outcome::Cancel => self.mode = self.prompt_return,
                Outcome::Submit(line) => {
                    self.mode = self.prompt_return;
                    commands.push(Command::Search(line));
                }
            },
            (Some(rect), _) => {
                let mut file = crate::welcome::display_name(&self.view.path);
                if self.annots.unsaved {
                    file.push_str(" [+]");
                }
                let linewise = self.selection.is_some_and(|s| s.linewise);
                let status = statusbar::Status {
                    mode: match (&self.placing, self.mode) {
                        (_, Mode::Visual) if linewise => "V-LINE",
                        (Some(pl), Mode::Place) => pl.purpose.label(),
                        _ => self.mode.label(),
                    },
                    file: &file,
                    page,
                    pages,
                    zoom_percent: vp.zoom * 100.0,
                    pending: self.keys.pending_display(),
                    search: self.search_status(),
                    message: match &self.confirm {
                        Some(c) => Some((c.question.as_str(), false)),
                        None => self.message.as_ref().map(|m| (m.text.as_str(), m.error)),
                    },
                    font_size,
                };
                commands.extend(statusbar::show(ui, rect, &status));
            }
            (None, _) => {
                // No status bar: show messages over the page area.
                if let Some(m) = &self.message {
                    ui.painter().with_clip_rect(area).text(
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
    }

    pub(super) fn show_outline_panel(
        &mut self,
        ctx: &egui::Context,
        rect: Rect,
        vp: &Viewport,
        commands: &mut Vec<Command>,
    ) {
        let font_size = self.config.font_size;
        let focused = self.mode == Mode::Outline;
        if !focused {
            // Pinned and idle: follow the page being read.
            let current = self.outline_entry_for(vp.centre_page(&self.view.layout));
            if current != self.outline.selected {
                self.outline.selected = current;
                self.outline.follow = true;
            }
        }
        let out = ui::outline::show(
            ctx,
            ui::outline::Panel {
                rect,
                items: &self.view.outline,
                selected: self.outline.selected,
                follow: self.outline.follow,
                focused,
                pinned: self.config.outline_pinned,
                font_size,
            },
        );
        self.outline.follow = false;
        if out.resized != 0.0 {
            self.config.outline_width = (rect.width() + out.resized).max(OUTLINE_MIN_WIDTH);
        }
        if let Some(i) = out.clicked {
            // Clicking jumps but keeps the panel open, to browse.
            self.outline.selected = i;
            if let Some(target) = self.view.outline[i].target.clone() {
                commands.push(Command::FollowLink(target));
            }
        }
    }
}
