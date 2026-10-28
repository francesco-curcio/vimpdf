//! The menu bar's contents, the commands behind it, and the document
//! properties window.

use eframe::egui::{Align2, Grid, RichText};

use super::*;
use crate::annotate::AnnotKind;
use crate::ui::menubar::{self, Item, Menu, MenuAction};

const RECENT_FILES: usize = 10;

impl VimpdfApp {
    /// The key shown next to a menu entry: the current binding (normal or
    /// visual mode), or a fallback such as a `:` command.
    fn key_label(&self, cmd: &Command, fallback: &str) -> String {
        self.keymap
            .key_for(cmd)
            .or_else(|| self.visual_keymap.key_for(cmd).map(|k| format!("v: {k}")))
            .unwrap_or_else(|| fallback.to_owned())
    }

    fn item(&self, label: &str, cmd: Command, fallback: &str) -> Item {
        let key = self.key_label(&cmd, fallback);
        Item::action(label, key, cmd)
    }

    fn toggle_item(&self, label: &str, cmd: Command, on: bool) -> Item {
        let key = self.key_label(&cmd, "");
        Item::toggle(label, key, cmd, on)
    }

    /// The menus, built from the current state (enabled entries, check
    /// marks, recent files, highlight colours, key bindings).
    pub(super) fn menus(&self) -> Vec<Menu> {
        let has_sel = self.selection.is_some();
        let has_marks = self.annots.list.iter().any(|a| a.kind == AnnotKind::Redact);
        let searched = !self.view.search.query.is_empty();

        let recent: Vec<Item> = crate::state::state_dir()
            .map(|d| crate::state::recent(&d, RECENT_FILES + 1))
            .unwrap_or_default()
            .into_iter()
            .filter(|p| *p != self.view.path)
            .take(RECENT_FILES)
            .enumerate()
            .map(|(i, p)| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // Underline the digit: 1-9 then 0.
                let digit = (i + 1) % 10;
                Item::action(&format!("&{digit}  {name}"), "", Command::Open(p))
            })
            .collect();
        let recent = if recent.is_empty() {
            vec![Item::action("(none yet)", "", Command::Cancel).enabled(false)]
        } else {
            recent
        };

        let colours: Vec<Item> = self
            .config
            .highlight_colors
            .iter()
            .map(|(letter, _)| {
                Item::action(
                    &format!("Colour &{letter}"),
                    format!("v: a{letter}"),
                    Command::Highlight(Some(*letter)),
                )
                .enabled(has_sel)
            })
            .collect();

        let file = Menu {
            label: "&File".into(),
            items: vec![
                self.item("&Open…", Command::EnterCommandLine("open ".into()), ":open"),
                Item::Submenu {
                    label: "Open &recent".into(),
                    items: recent,
                },
                self.item("R&eload", Command::Reload, ":e"),
                Item::Separator,
                self.item("&Save annotations", Command::Write(None), ":w")
                    .enabled(self.annots.unsaved),
                self.item(
                    "Save &as… (and switch to it)",
                    Command::EnterCommandLine("saveas ".into()),
                    ":saveas",
                ),
                self.item(
                    "Save a cop&y…",
                    Command::EnterCommandLine("w ".into()),
                    ":w <path>",
                ),
                self.item(
                    "Open a&nnotated copy",
                    Command::OpenAnnotatedCopy,
                    ":annotated",
                )
                .enabled(self.persist.annotated_copy.is_some()),
                self.item(
                    "A&pply redactions…",
                    Command::EnterCommandLine("redact ".into()),
                    ":redact",
                )
                .enabled(has_marks),
                Item::Separator,
                self.item("Document &info…", Command::ToggleProperties, ":info"),
                Item::Separator,
                self.item("&Quit", Command::Quit, ":q"),
            ],
        };
        let edit = Menu {
            label: "&Edit".into(),
            items: vec![
                self.item("&Undo", Command::Undo, ":undo"),
                Item::Separator,
                self.item("&Copy", Command::Yank, "y").enabled(has_sel),
                self.item("&Select text", Command::Visual { linewise: false }, "v"),
                Item::Submenu {
                    label: "Hi&ghlight".into(),
                    items: colours,
                },
                self.item("Co&mment…", Command::Comment, "c")
                    .enabled(has_sel),
                self.item("Add stic&ky note", Command::PlaceNote, "C"),
                self.item("Add text &box", Command::PlaceText, "T"),
                self.item("Annotatio&ns…", Command::ToggleAnnotations, ":annotations"),
                Item::Separator,
                self.item("Mark for &redaction", Command::RedactSelection, "x")
                    .enabled(has_sel),
                self.item("Mark &area for redaction", Command::RedactArea, "X"),
                self.item(
                    "Mark search ma&tches for redaction",
                    Command::RedactMatches,
                    ":redact-matches",
                )
                .enabled(searched),
                Item::Separator,
                self.item("&Find…", Command::EnterSearch(String::new()), "/"),
                self.item("Find ne&xt", Command::SearchNext(1), "n")
                    .enabled(searched),
                self.item("Find pre&vious", Command::SearchPrev(1), "N")
                    .enabled(searched),
                Item::Separator,
                self.item("Edit c&onfig file", Command::EditConfig, ":config"),
                self.item("Reloa&d config", Command::Source, ":source"),
            ],
        };
        let zoom = self.viewport.as_ref().map(|vp| vp.fit);
        let view = Menu {
            label: "&View".into(),
            items: vec![
                self.item("Zoom &in", Command::ZoomIn(1), "+"),
                self.item("Zoom &out", Command::ZoomOut(1), "-"),
                self.item("&Actual size (100%)", Command::ZoomReset, "="),
                Item::Separator,
                self.toggle_item(
                    "Fit &width",
                    Command::FitWidth,
                    zoom == Some(FitMode::Width),
                ),
                self.toggle_item("Fit &page", Command::FitPage, zoom == Some(FitMode::Page)),
                Item::Separator,
                self.toggle_item(
                    "&Recolor (dark mode)",
                    Command::ToggleRecolor,
                    self.config.recolor,
                ),
                self.toggle_item(
                    "Ou&tline",
                    Command::ToggleOutline,
                    self.mode == Mode::Outline,
                ),
                self.toggle_item(
                    "Annotatio&ns panel",
                    Command::ToggleAnnotations,
                    self.mode == Mode::Annotations,
                ),
                Item::Separator,
                self.toggle_item("&Menu bar", Command::ToggleMenubar, self.config.menubar),
                self.toggle_item("Tool&bar", Command::ToggleToolbar, self.config.toolbar),
                self.toggle_item(
                    "&Status bar",
                    Command::ToggleStatusbar,
                    self.config.statusbar,
                ),
                self.item("&Fullscreen", Command::ToggleFullscreen, ""),
                Item::Separator,
                self.item("Keys and comman&ds", Command::ToggleHelp, "?"),
            ],
        };
        vec![file, edit, view]
    }

    pub(super) fn open_menu(&mut self) {
        let menus = self.menus();
        self.menu_state.open(&menus, 0);
        self.keys.reset();
        self.menu_return = if self.mode == Mode::Visual {
            Mode::Visual
        } else {
            Mode::Normal
        };
        self.mode = Mode::Menu;
    }

    pub(super) fn close_menu(&mut self) {
        self.menu_state = Default::default();
        if self.mode == Mode::Menu {
            self.mode = self.menu_return;
        }
    }

    /// A key while a menu is open.
    pub(super) fn menu_key(&mut self, tok: &str) -> Option<Command> {
        let menus = self.menus();
        match self.menu_state.key(&menus, tok) {
            MenuAction::None => None,
            MenuAction::Close => {
                self.close_menu();
                None
            }
            MenuAction::Run(cmd) => {
                self.close_menu();
                Some(cmd)
            }
        }
    }

    /// Draw the bar (if shown, or while a menu is open from the keyboard).
    pub(super) fn show_menubar(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        rect: Rect,
    ) -> Option<Command> {
        let menus = self.menus();
        let was_open = self.menu_state.is_open();
        let action = menubar::show(
            ctx,
            ui,
            rect,
            &menus,
            &mut self.menu_state,
            self.config.font_size,
        );
        if !was_open && self.menu_state.is_open() {
            // Opened with the mouse.
            self.menu_return = if self.mode == Mode::Visual {
                Mode::Visual
            } else {
                Mode::Normal
            };
            self.keys.reset();
            self.mode = Mode::Menu;
        }
        match action {
            MenuAction::None => None,
            MenuAction::Close => {
                self.close_menu();
                None
            }
            MenuAction::Run(cmd) => {
                self.close_menu();
                Some(cmd)
            }
        }
    }

    /// Open the config file in the system's text editor, creating it from
    /// the documented example first if it doesn't exist.
    pub(super) fn edit_config(&mut self) {
        let Some(path) = self.config_path.clone() else {
            return self.error("No config file location (HOME not set?)");
        };
        if !path.exists() {
            let created = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, crate::config::EXAMPLE));
            if let Err(e) = created {
                return self.error(format!("Cannot create {}: {e}", path.display()));
            }
        }
        let (program, args): (&str, Vec<&std::ffi::OsStr>) = if cfg!(target_os = "macos") {
            ("open", vec!["-t".as_ref(), path.as_os_str()])
        } else {
            ("xdg-open", vec![path.as_os_str()])
        };
        match std::process::Command::new(program).args(args).spawn() {
            Ok(_) => self.info(format!(
                "Opened {} in your editor — :source reloads it",
                path.display()
            )),
            Err(e) => self.error(format!("Cannot run {program}: {e}")),
        }
    }

    /// The document properties window (`:info`): closes with Esc, q, Enter.
    pub(super) fn show_properties(&mut self, ctx: &egui::Context) {
        let path = &self.view.path;
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let annots = &self.annots.list;
        let count = |k: AnnotKind| annots.iter().filter(|a| a.kind == k).count();
        let mut rows: Vec<(String, String)> = vec![
            ("File".into(), path.display().to_string()),
            ("Size".into(), human_size(size)),
            ("Pages".into(), self.view.page_count().to_string()),
        ];
        rows.extend(
            self.view
                .metadata
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone())),
        );
        rows.push((
            "Annotations".into(),
            format!(
                "{} highlights, {} notes, {} redaction marks, {} other{}",
                count(AnnotKind::Highlight),
                count(AnnotKind::Note),
                count(AnnotKind::Redact),
                count(AnnotKind::Other),
                if self.annots.unsaved {
                    " (unsaved changes)"
                } else {
                    ""
                }
            ),
        ));
        let font = egui::FontId::proportional(self.config.font_size);
        let mut open = self.show_props;
        egui::Window::new("Document info")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                Grid::new("vimpdf-props").striped(true).show(ui, |ui| {
                    for (k, v) in &rows {
                        ui.label(RichText::new(k).font(font.clone()).color(ui::BAR_DIM));
                        ui.label(RichText::new(v).font(font.clone()));
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Esc, q or Enter closes this.").color(ui::BAR_DIM));
            });
        self.show_props = open;
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / f64::from(1 << 20)),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{b} bytes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 bytes");
        assert_eq!(human_size(4096), "4 KB");
        assert_eq!(human_size(3 * 1024 * 1024 + 300_000), "3.3 MB");
    }
}
