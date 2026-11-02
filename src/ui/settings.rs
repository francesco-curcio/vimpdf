//! The Settings window: every option of the config file, with a control
//! each. Changes apply at once (like `:set`); Save writes into the config
//! file every option whose value, as shown, differs from the file's (so
//! also what `<C-r>`, `<C-t>` or `:set` changed before), Cancel puts
//! everything back. A second page lists the key bindings
//! (`super::keybindings`); gt / gT switch pages.

use eframe::egui::{self, Align2, Grid, RichText, ScrollArea, Vec2};
use toml::Value;

use super::BAR_DIM;
use super::keybindings::KeysPage;
use crate::config::{Config, KeyOverrides};
use crate::input::command::Command;

enum Kind {
    Bool,
    Number {
        min: f64,
        max: f64,
        speed: f64,
    },
    Color,
    Choice(&'static [&'static str]),
    /// Applied when the field loses focus (a font is looked up then).
    Text,
    /// One colour per highlight letter.
    HighlightColors,
    /// One of the highlight letters.
    HighlightLetter,
}

struct Setting {
    key: &'static str,
    label: &'static str,
    kind: Kind,
    hint: &'static str,
}

const fn s(key: &'static str, label: &'static str, kind: Kind, hint: &'static str) -> Setting {
    Setting {
        key,
        label,
        kind,
        hint,
    }
}

const fn n(min: f64, max: f64, speed: f64) -> Kind {
    Kind::Number { min, max, speed }
}

const MODIFIERS: &[&str] = &["cmd", "ctrl", "alt", "shift"];

/// The options by section, in the order of the configuration docs. Ranges
/// stay within what `Config::set` accepts.
const SECTIONS: &[(&str, &[Setting])] = &[
    (
        "Interface",
        &[
            s(
                "ui.font_family",
                "Font",
                Kind::Text,
                "Font of the bars, command line and help. \"monospace\" is the system's monospace font.",
            ),
            s(
                "ui.font_size",
                "Font size",
                n(6.0, 72.0, 0.2),
                "Also scales the bars.",
            ),
            s(
                "ui.menubar",
                "Menu bar",
                Kind::Bool,
                "File / Edit / View menus (gm opens them even when hidden).",
            ),
            s(
                "ui.tabbar",
                "Tab bar",
                Kind::Bool,
                "gt / gT switch tabs either way.",
            ),
            s("ui.toolbar", "Toolbar", Kind::Bool, "<C-t> toggles it."),
            s("ui.statusbar", "Status bar", Kind::Bool, ""),
            s(
                "ui.scrollbar",
                "Scrollbar",
                Kind::Bool,
                "Beside the pages, on the right.",
            ),
            s(
                "ui.start_maximised",
                "Start maximised",
                Kind::Bool,
                "Takes effect at the next launch.",
            ),
            s(
                "ui.recolor",
                "Start in dark mode",
                Kind::Bool,
                "Ticked while dark mode is on (<C-r> toggles it); Save makes vimpdf start that way.",
            ),
            s(
                "ui.recolor_keep_hue",
                "Dark mode keeps colours",
                Kind::Bool,
                "Off: dark mode turns the page monochrome.",
            ),
            s(
                "ui.auto_reload",
                "Reload changed files",
                Kind::Bool,
                "Reload the document when the file changes on disk (e.g. LaTeX).",
            ),
            s(
                "ui.single_instance",
                "Open files as tabs",
                Kind::Bool,
                "Files opened while vimpdf runs become tabs of the running window; off: each gets its own window. Takes effect at the next launch.",
            ),
            s(
                "ui.restore_tabs",
                "Reopen last tabs",
                Kind::Bool,
                "At launch, reopen the tabs that were open when vimpdf last closed.",
            ),
        ],
    ),
    (
        "Colours",
        &[
            s(
                "colors.page_background",
                "Page (while rendering)",
                Kind::Color,
                "",
            ),
            s(
                "colors.recolor_dark",
                "Dark mode background",
                Kind::Color,
                "White maps here.",
            ),
            s(
                "colors.recolor_light",
                "Dark mode text",
                Kind::Color,
                "Black maps here.",
            ),
            s("colors.search_highlight", "Search matches", Kind::Color, ""),
            s("colors.selection", "Selection", Kind::Color, ""),
            s("colors.caret", "Caret", Kind::Color, "Visual mode caret."),
            s(
                "colors.hint_background",
                "Link labels",
                Kind::Color,
                "Labels shown by f.",
            ),
        ],
    ),
    (
        "Scrolling and zoom",
        &[
            s(
                "scroll.step",
                "Scroll step",
                n(1.0, 1000.0, 1.0),
                "Distance of j / k, in screen points.",
            ),
            s("scroll.smooth", "Smooth scrolling", Kind::Bool, ""),
            s(
                "zoom.step",
                "Zoom step",
                n(1.01, 4.0, 0.01),
                "Factor of + / -.",
            ),
            s(
                "zoom.default_fit",
                "New documents",
                Kind::Choice(&["width", "page", "none"]),
                "Fit the width, the page, or open at 100%.",
            ),
        ],
    ),
    (
        "Mouse",
        &[
            s(
                "mouse.drag",
                "Dragging",
                Kind::Choice(&["select", "pan"]),
                "What dragging on a page does.",
            ),
            s(
                "mouse.pan_modifier",
                "Pan with",
                Kind::Choice(MODIFIERS),
                "Held while dragging, pans (with select) or selects (with pan).",
            ),
            s(
                "mouse.zoom_modifier",
                "Zoom with",
                Kind::Choice(MODIFIERS),
                "Held while scrolling, zooms.",
            ),
            s(
                "mouse.copy_on_select",
                "Copy on select",
                Kind::Bool,
                "Copy text as soon as it is selected.",
            ),
            s(
                "mouse.link_preview",
                "Link preview",
                Kind::Bool,
                "Rest the pointer on a link inside the document to see where it leads.",
            ),
            s(
                "mouse.hide_after",
                "Hide pointer after",
                n(0.0, 60.0, 0.1),
                "Idle seconds over the pages; 0 never hides it.",
            ),
        ],
    ),
    (
        "Outline",
        &[
            s(
                "outline.pinned",
                "Keep open",
                Kind::Bool,
                "The table of contents stays beside the pages.",
            ),
            s("outline.width", "Width", n(160.0, 2000.0, 1.0), ""),
        ],
    ),
    (
        "Highlights and text",
        &[
            s(
                "highlight.colors",
                "Highlight colours",
                Kind::HighlightColors,
                "a<letter> in visual mode.",
            ),
            s(
                "highlight.default",
                "Default colour",
                Kind::HighlightLetter,
                "Used by c (comment) and aa.",
            ),
            s(
                "highlight.author",
                "Author",
                Kind::Text,
                "Written into new annotations; empty: your login name.",
            ),
            s(
                "text.size",
                "Text box size",
                n(4.0, 72.0, 0.2),
                "Font size of new text boxes (T).",
            ),
            s("text.color", "Text box colour", Kind::Color, ""),
        ],
    ),
];

pub enum Outcome {
    /// Write these options into the config file, and the key bindings if
    /// they changed.
    Save {
        options: Vec<(&'static str, Value)>,
        keys: Option<KeyOverrides>,
    },
    /// Put the config back as it was when the window opened.
    Cancel(Box<Config>),
    /// Open the config file in an editor (key bindings live there).
    EditFile,
}

pub struct SettingsWindow {
    /// The config when the window opened, for Cancel.
    before: Config,
    /// What the config file says (defaults for what it leaves out).
    file: Config,
    /// Text fields being edited, applied when they lose focus.
    texts: std::collections::HashMap<&'static str, String>,
    /// Scrolled by the keyboard like the help overlay.
    pub scroll: super::help::HelpScroll,
    /// Showing the key bindings page instead of the options.
    on_keys: bool,
    keys: KeysPage,
    /// The file's key bindings, as config entries.
    file_keys: KeyOverrides,
}

impl SettingsWindow {
    pub fn new(config: &Config, file: Config) -> Self {
        let file_keys = KeysPage::new(file.normal_keymap().0, file.visual_keymap().0).overrides();
        Self {
            before: config.clone(),
            file,
            texts: Default::default(),
            scroll: Default::default(),
            on_keys: false,
            keys: KeysPage::new(config.normal_keymap().0, config.visual_keymap().0),
            file_keys,
        }
    }

    /// Open on the key bindings page.
    pub fn show_keys(&mut self) {
        self.on_keys = true;
    }

    /// The key bindings, when they don't match the file's.
    fn unsaved_keys(&self) -> Option<KeyOverrides> {
        let keys = self.keys.overrides();
        (keys != self.file_keys).then_some(keys)
    }

    /// A key typed while the window has the keyboard; `false` sends it on
    /// to the keymap (and back to `command`).
    pub fn key(&mut self, tok: &str) -> bool {
        self.on_keys && self.keys.key(tok)
    }

    /// Whether the key bindings page is waiting for keys to bind (they
    /// must not go through the keymap).
    pub fn recording(&self) -> bool {
        self.on_keys && self.keys.recording()
    }

    /// A command from the keymap: gt / gT switch pages, the rest scroll.
    pub fn command(&mut self, cmd: &Command, step: f32) {
        match cmd {
            Command::NextTab | Command::PrevTab(_) | Command::GotoTab(_) => {
                self.on_keys = !self.on_keys;
            }
            _ if self.on_keys => {
                self.keys.command(cmd);
            }
            _ => {
                self.scroll.key(cmd, step);
            }
        }
    }

    /// Put the key bindings into `config`, if they changed; true then (the
    /// app rebuilds its keymaps).
    pub fn apply_keys(&mut self, config: &mut Config) -> bool {
        if !self.keys.take_changed() {
            return false;
        }
        let keys = self.keys.overrides();
        config.normal_keys = keys.normal;
        config.visual_keys = keys.visual;
        true
    }

    /// Options whose value in `config` isn't what the file gives.
    fn unsaved(&self, config: &Config) -> Vec<(&'static str, Value)> {
        crate::config::OPTIONS
            .iter()
            .filter_map(|k| config.get(k).map(|v| (*k, v)))
            .filter(|(k, v)| self.file.get(k).as_ref() != Some(v))
            .collect()
    }

    /// Draw the window, changing `config` as controls change. Its own text
    /// keeps the size it opened with, so dragging the font size doesn't
    /// move the window under the pointer.
    pub fn show(&mut self, ctx: &egui::Context, config: &mut Config) -> Option<Outcome> {
        let font_size = self.before.font_size;
        // Esc in a dropdown or colour picker closes just that.
        let popup_open = egui::Popup::is_any_open(ctx);
        let defaults = Config::default();
        let mut outcome = None;
        let mut open = true;
        let height = (ctx.content_rect().height() * 0.8).max(200.0);
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.style_mut().override_font_id =
                    Some(egui::FontId::proportional(font_size.min(18.0)));
                ui.horizontal(|ui| {
                    if ui.selectable_label(!self.on_keys, "Options").clicked() {
                        self.on_keys = false;
                    }
                    if ui.selectable_label(self.on_keys, "Key bindings").clicked() {
                        self.on_keys = true;
                    }
                    ui.label(RichText::new("gt / gT switch").color(BAR_DIM).small());
                });
                ui.separator();
                if self.on_keys {
                    // Same size as the options page, so switching doesn't
                    // resize the window.
                    ui.set_min_width(font_size.min(18.0) * 40.0);
                    self.keys
                        .show(ui, font_size.min(18.0), height - 8.0 * font_size);
                } else {
                let mut area = ScrollArea::vertical().max_height(height - 6.0 * font_size);
                if let Some(offset) = self.scroll.take_requested() {
                    area = area.vertical_scroll_offset(offset);
                }
                let out = area.show(ui, |ui| {
                    for (title, settings) in SECTIONS {
                        ui.add_space(6.0);
                        ui.label(RichText::new(*title).strong());
                        Grid::new(("vimpdf-settings", *title))
                            .num_columns(3)
                            .spacing([16.0, 6.0])
                            .min_col_width(font_size * 12.0)
                            .show(ui, |ui| {
                                for setting in *settings {
                                    self.row(ui, setting, config, &defaults);
                                    ui.end_row();
                                }
                            });
                    }
                });
                self.scroll.record(&out);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let unsaved = self.unsaved(config);
                    let unsaved_keys = self.unsaved_keys();
                    let save = ui
                        .add_enabled(
                            !unsaved.is_empty() || unsaved_keys.is_some(),
                            egui::Button::new("Save to config file"),
                        )
                        .on_hover_text(if cfg!(target_os = "macos") { "Cmd+S" } else { "Ctrl+S" })
                        .on_disabled_hover_text("The config file already says all this");
                    // Cmd+S (Ctrl+S elsewhere) saves too.
                    let save_key = ui.input_mut(|i| {
                        i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)
                    });
                    let can_save = !unsaved.is_empty() || unsaved_keys.is_some();
                    if save.clicked() || (save_key && can_save && !self.keys.recording()) {
                        outcome = Some(Outcome::Save {
                            options: unsaved,
                            keys: unsaved_keys,
                        });
                    }
                    if ui.button("Cancel").clicked() {
                        outcome = Some(Outcome::Cancel(Box::new(self.before.clone())));
                    }
                    if ui
                        .button("Open config file")
                        .on_hover_text("Edit the config file in your editor (Cancel these changes first, or Save them)")
                        .clicked()
                    {
                        outcome = Some(Outcome::EditFile);
                    }
                });
                let hint = if self.on_keys {
                    "j k pick a command, h l one of its keys · a / Enter add a key · \
                     x remove it · r reset · v normal / visual · / filter"
                } else {
                    "Changes apply now; Save (Cmd+S / Ctrl+S) keeps them for next time. \
                     j k, <C-d> <C-u>, gg G scroll."
                };
                ui.label(RichText::new(hint).color(BAR_DIM).small());
            });
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape))
            && !ctx.text_edit_focused()
            && !popup_open
            && !self.keys.take_esc_used();
        if outcome.is_none() && (!open || escape) {
            outcome = Some(Outcome::Cancel(Box::new(self.before.clone())));
        }
        outcome
    }

    fn row(
        &mut self,
        ui: &mut egui::Ui,
        setting: &Setting,
        config: &mut Config,
        defaults: &Config,
    ) {
        let key = setting.key;
        let label = ui.label(setting.label);
        if !setting.hint.is_empty() {
            label.on_hover_text(setting.hint);
        }
        let Some(value) = config.get(key) else {
            return;
        };
        let new = control(ui, setting, &value, config, &mut self.texts);
        if let Some(v) = new.filter(|v| *v != value) {
            let _ = config.set(key, &v);
        }
        let default = defaults.get(key);
        let is_default = config.get(key) == default;
        let reset = ui.add_visible(!is_default, egui::Button::new("↺").small());
        if reset.on_hover_text("Back to the default").clicked()
            && let Some(d) = default
            && config.set(key, &d).is_ok()
        {
            self.texts.remove(key);
        }
    }
}

/// The control for one setting; returns the new value when it was changed.
fn control(
    ui: &mut egui::Ui,
    setting: &Setting,
    value: &Value,
    config: &Config,
    texts: &mut std::collections::HashMap<&'static str, String>,
) -> Option<Value> {
    let key = setting.key;
    match &setting.kind {
        Kind::Bool => {
            let mut on = value.as_bool().unwrap_or_default();
            ui.checkbox(&mut on, "")
                .changed()
                .then_some(Value::Boolean(on))
        }
        Kind::Number { min, max, speed } => {
            let mut v = value
                .as_float()
                .or_else(|| value.as_integer().map(|i| i as f64))
                .unwrap_or_default();
            let decimals = if *speed < 0.1 {
                2
            } else if *speed < 1.0 {
                1
            } else {
                0
            };
            ui.add(
                egui::DragValue::new(&mut v)
                    .range(*min..=*max)
                    .speed(*speed)
                    .max_decimals(decimals),
            )
            .changed()
            .then(|| number(v, decimals))
        }
        Kind::Color => color(ui, value.as_str().unwrap_or_default()).map(Value::String),
        Kind::Choice(choices) => choice(ui, key, value.as_str().unwrap_or_default(), choices),
        Kind::Text => {
            let text = texts
                .entry(key)
                .or_insert_with(|| value.as_str().unwrap_or_default().to_owned());
            let response = ui.add(egui::TextEdit::singleline(text).desired_width(180.0));
            if !response.has_focus() && !response.lost_focus() {
                // Follow changes made elsewhere (↺, Cancel, :set).
                value.as_str().unwrap_or_default().clone_into(text);
            }
            response
                .lost_focus()
                .then(|| Value::String(text.trim().to_owned()))
        }
        Kind::HighlightColors => {
            let table = value.as_table()?;
            let mut changed = None;
            // A grid, three per line: wrapped widgets in a grid cell would
            // spill over the rows below (the row keeps one line's height).
            Grid::new("vimpdf-highlight-colours")
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    for (i, (letter, c)) in table.iter().enumerate() {
                        ui.label(RichText::new(letter).monospace());
                        if let Some(new) = color(ui, c.as_str().unwrap_or_default()) {
                            let mut t = table.clone();
                            t.insert(letter.clone(), Value::String(new));
                            changed = Some(Value::Table(t));
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            changed
        }
        Kind::HighlightLetter => {
            let letters: Vec<String> = config
                .highlight_colors
                .iter()
                .map(|(k, _)| k.to_string())
                .collect();
            let letters: Vec<&str> = letters.iter().map(String::as_str).collect();
            choice(ui, key, value.as_str().unwrap_or_default(), &letters)
        }
    }
}

fn number(v: f64, decimals: usize) -> Value {
    if decimals == 0 {
        Value::Integer(v.round() as i64)
    } else {
        let scale = 10f64.powi(decimals as i32);
        Value::Float((v * scale).round() / scale)
    }
}

fn choice(ui: &mut egui::Ui, key: &str, current: &str, choices: &[&str]) -> Option<Value> {
    let mut picked = None;
    egui::ComboBox::from_id_salt(("vimpdf-setting", key))
        .selected_text(current)
        .show_ui(ui, |ui| {
            for c in choices {
                if ui.selectable_label(*c == current, *c).clicked() && *c != current {
                    picked = Some(Value::String((*c).to_owned()));
                }
            }
        });
    picked
}

/// A colour button with a picker; returns the new "#rrggbb[aa]".
fn color(ui: &mut egui::Ui, hex: &str) -> Option<String> {
    let mut rgba = crate::config::Color::parse(hex).ok()?.0;
    ui.color_edit_button_srgba_unmultiplied(&mut rgba)
        .changed()
        .then(|| crate::config::Color(rgba).to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        ctx: &egui::Context,
        w: &mut SettingsWindow,
        c: &mut Config,
        escape: bool,
    ) -> Option<Outcome> {
        let events = if escape {
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]
        } else {
            Vec::new()
        };
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut outcome = None;
        // Nothing is painted: drop the font atlas upload.
        ctx.run_pass(input, |ctx| outcome = w.show(ctx, c))
            .textures_delta
            .clear();
        outcome
    }

    #[test]
    fn shows_every_option_and_escape_puts_the_config_back() {
        let ctx = egui::Context::default();
        let mut config = Config::default();
        let mut w = SettingsWindow::new(&config, Config::default());
        assert!(frame(&ctx, &mut w, &mut config, false).is_none());
        // A change made while it's open (as a control would).
        config.toolbar = true;
        assert!(frame(&ctx, &mut w, &mut config, false).is_none());
        match frame(&ctx, &mut w, &mut config, true) {
            Some(Outcome::Cancel(before)) => assert_eq!(*before, Config::default()),
            _ => panic!("expected Cancel"),
        }
    }

    #[test]
    fn saves_what_differs_from_the_file_however_it_was_changed() {
        let file = Config {
            zoom_step: 1.5,
            ..Config::default()
        };
        // Dark mode switched on with <C-r> before opening the window.
        let mut config = file.clone();
        config.recolor = true;
        let w = SettingsWindow::new(&config, file);
        assert_eq!(
            w.unsaved(&config),
            vec![("ui.recolor", Value::Boolean(true))]
        );
        config.zoom_step = 1.1;
        assert_eq!(
            w.unsaved(&config).len(),
            2,
            "back to the default, unlike the file"
        );
        config.recolor = false;
        config.zoom_step = 1.5;
        assert!(w.unsaved(&config).is_empty());
    }

    #[test]
    fn key_bindings_page_changes_the_config_and_counts_as_unsaved() {
        let mut config = Config::default();
        let mut w = SettingsWindow::new(&config, Config::default());
        // Its keys only work on its page; gt gets there.
        assert!(!w.key("a"));
        w.command(&Command::NextTab, 1.0);
        assert!(w.key("/"));
        w.keys = KeysPage::new(config.normal_keymap().0, config.visual_keymap().0);
        for tok in ["a", "<C-x>", "<CR>"] {
            assert!(w.key(tok));
        }
        assert!(w.apply_keys(&mut config));
        assert_eq!(config.normal_keys.len(), 1);
        assert!(w.unsaved_keys().is_some());
        assert!(!w.apply_keys(&mut config), "nothing new");
    }

    #[test]
    fn every_option_is_in_the_window_once() {
        let mut keys: Vec<&str> = SECTIONS
            .iter()
            .flat_map(|(_, settings)| settings.iter().map(|s| s.key))
            .collect();
        let total = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), total, "an option is listed twice");
        let mut options = crate::config::OPTIONS.to_vec();
        options.sort();
        assert_eq!(keys, options);
    }

    #[test]
    fn every_value_the_window_can_produce_is_accepted() {
        let mut c = Config::default();
        for (_, settings) in SECTIONS {
            for setting in *settings {
                let values: Vec<Value> = match &setting.kind {
                    Kind::Number { min, max, speed } => {
                        let d = if *speed < 0.1 {
                            2
                        } else if *speed < 1.0 {
                            1
                        } else {
                            0
                        };
                        vec![number(*min, d), number(*max, d)]
                    }
                    Kind::Choice(choices) => {
                        choices.iter().map(|c| Value::String((*c).into())).collect()
                    }
                    _ => continue,
                };
                for v in values {
                    assert_eq!(c.set(setting.key, &v), Ok(()), "{} = {v}", setting.key);
                }
            }
        }
    }
}
