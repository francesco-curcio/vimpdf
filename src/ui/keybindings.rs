//! The Settings window's key bindings page: every command with its keys,
//! for normal and visual mode. Keys are added by pressing them, removed,
//! or put back to the defaults; the changes are kept as the config's
//! `[keys.*]` entries (what differs from the defaults).
//!
//! Keyboard: j / k pick a command, h / l one of its keys, a or Enter adds a
//! key (press it, then Enter), x removes the picked key, r resets the
//! command, v switches mode, / filters.

use eframe::egui::{self, Align, Frame, RichText, ScrollArea, Sense, Stroke};

use super::{ACCENT, BAR_DIM, ERROR};
use crate::config::KeyOverrides;
use crate::input::command::{self, Command};
use crate::input::keymap::{KeySeq, Keymap, seq_to_string};

/// A change asked for while drawing, made after.
type Action = Box<dyn FnOnce(&mut KeysPage)>;

/// Longest sequence the recorder takes.
const MAX_KEYS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Visual,
}

pub struct KeysPage {
    defaults: [Keymap; 2],
    current: [Keymap; 2],
    mode: Mode,
    filter: String,
    focus_filter: bool,
    /// Picked row and key in it.
    selected: usize,
    chip: usize,
    /// Keys pressed so far for a new binding of this command.
    recording: Option<(Command, KeySeq)>,
    /// Scroll the picked row into view on the next frame.
    follow: bool,
    /// The bindings changed since the last `take_changed`.
    changed: bool,
    /// Esc was used here this frame (so it doesn't close the window).
    esc_used: bool,
}

/// "scroll_down" → "Scroll down".
fn label(cmd: &Command) -> String {
    let name = cmd.describe().replace('_', " ");
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => name,
    }
}

impl KeysPage {
    pub fn new(normal: Keymap, visual: Keymap) -> Self {
        Self {
            defaults: [Keymap::default_normal(), Keymap::default_visual()],
            current: [normal, visual],
            mode: Mode::Normal,
            filter: String::new(),
            focus_filter: false,
            selected: 0,
            chip: 0,
            recording: None,
            follow: false,
            changed: false,
            esc_used: false,
        }
    }

    fn index(&self) -> usize {
        match self.mode {
            Mode::Normal => 0,
            Mode::Visual => 1,
        }
    }

    fn keymap(&self) -> &Keymap {
        &self.current[self.index()]
    }

    fn keymap_mut(&mut self) -> &mut Keymap {
        &mut self.current[self.index()]
    }

    fn defaults(&self) -> &Keymap {
        &self.defaults[self.index()]
    }

    /// The bindings as config entries.
    pub fn overrides(&self) -> KeyOverrides {
        KeyOverrides {
            normal: Keymap::overrides(&self.defaults[0], &self.current[0]),
            visual: Keymap::overrides(&self.defaults[1], &self.current[1]),
        }
    }

    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    pub fn take_esc_used(&mut self) -> bool {
        std::mem::take(&mut self.esc_used)
    }

    pub fn recording(&self) -> bool {
        self.recording.is_some()
    }

    /// The commands listed for the mode, sorted by label, filtered. Normal
    /// mode leaves out what only works on a selection (unless bound there).
    fn rows(&self) -> Vec<Command> {
        let visual = self.mode == Mode::Visual;
        let (normal_defaults, visual_defaults) = (&self.defaults[0], &self.defaults[1]);
        let mut all: Vec<Command> = if visual {
            command::bindable(true)
                .into_iter()
                .filter(|c| matches!(c, Command::Move(..)))
                .chain(visual_defaults.commands())
                .collect()
        } else {
            let normal = normal_defaults.commands();
            command::bindable(false)
                .into_iter()
                .filter(|c| normal.contains(c) || visual_defaults.keys_for(c).is_empty())
                .chain(normal.clone())
                .collect()
        };
        all.extend(self.keymap().commands());
        let mut rows: Vec<(String, Command)> = Vec::new();
        for cmd in all {
            if !rows.iter().any(|(_, c)| *c == cmd) {
                rows.push((label(&cmd), cmd));
            }
        }
        let filter = self.filter.trim().to_lowercase();
        if !filter.is_empty() {
            rows.retain(|(l, c)| {
                l.to_lowercase().contains(&filter)
                    || c.describe().contains(&filter)
                    || self
                        .keymap()
                        .keys_for(c)
                        .iter()
                        .any(|k| seq_to_string(k).to_lowercase() == filter)
            });
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows.into_iter().map(|(_, c)| c).collect()
    }

    fn selected_row(&self) -> Option<Command> {
        let rows = self.rows();
        rows.get(self.selected.min(rows.len().saturating_sub(1)))
            .cloned()
    }

    fn move_selection(&mut self, by: i64) {
        let last = self.rows().len().saturating_sub(1) as i64;
        self.selected = (self.selected as i64 + by).clamp(0, last) as usize;
        self.chip = 0;
        self.follow = true;
    }

    fn start_recording(&mut self) {
        if let Some(cmd) = self.selected_row() {
            self.recording = Some((cmd, Vec::new()));
        }
    }

    /// Remove the picked key of the picked command.
    fn remove_chip(&mut self) {
        let Some(cmd) = self.selected_row() else {
            return;
        };
        let keys = self.keymap().keys_for(&cmd);
        if let Some(seq) = keys.get(self.chip.min(keys.len().saturating_sub(1))) {
            let seq = seq.clone();
            self.unbind(&seq);
            self.chip = self.chip.min(keys.len().saturating_sub(2));
        }
    }

    fn unbind(&mut self, seq: &[String]) {
        self.keymap_mut().unbind(seq);
        self.changed = true;
    }

    /// Give `cmd` its default keys back (and only those).
    fn reset(&mut self, cmd: &Command) {
        for seq in self.keymap().keys_for(cmd) {
            self.keymap_mut().unbind(&seq);
        }
        for seq in self.defaults().keys_for(cmd) {
            self.keymap_mut().bind_parsed(seq, cmd.clone());
        }
        self.changed = true;
    }

    fn reset_all(&mut self) {
        self.current = self.defaults.clone();
        self.changed = true;
    }

    fn switch_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Normal => Mode::Visual,
            Mode::Visual => Mode::Normal,
        };
        self.selected = 0;
        self.chip = 0;
        self.follow = true;
    }

    /// A key typed while the page shows (and no text field has focus).
    /// Returns whether it was used; the others go through the keymap and
    /// come back as `command`.
    pub fn key(&mut self, tok: &str) -> bool {
        if let Some((cmd, seq)) = self.recording.as_mut() {
            match tok {
                "<Esc>" => {
                    self.recording = None;
                    self.esc_used = true;
                }
                "<CR>" if !seq.is_empty() => {
                    let (cmd, seq) = (cmd.clone(), seq.clone());
                    self.recording = None;
                    self.bind(seq, cmd);
                }
                _ if seq.len() < MAX_KEYS => seq.push(tok.to_owned()),
                _ => {}
            }
            return true;
        }
        match tok {
            "j" | "<Down>" => self.move_selection(1),
            "k" | "<Up>" => self.move_selection(-1),
            "h" | "<Left>" => self.chip = self.chip.saturating_sub(1),
            "l" | "<Right>" => {
                let keys = self
                    .selected_row()
                    .map_or(0, |c| self.keymap().keys_for(&c).len());
                self.chip = (self.chip + 1).min(keys.saturating_sub(1));
            }
            "a" | "o" | "i" | "<CR>" => self.start_recording(),
            "x" | "d" | "<Del>" | "<BS>" => self.remove_chip(),
            "r" | "u" => {
                if let Some(cmd) = self.selected_row() {
                    self.reset(&cmd);
                }
            }
            "v" => self.switch_mode(),
            "/" => self.focus_filter = true,
            "<Esc>" if !self.filter.is_empty() => {
                self.filter.clear();
                self.esc_used = true;
            }
            _ => return false,
        }
        true
    }

    /// A command from the keymap (for gg, G, <C-d>, <C-u>…).
    pub fn command(&mut self, cmd: &Command) -> bool {
        match cmd {
            Command::GotoFirst => self.move_selection(i64::MIN / 2),
            Command::GotoLast => self.move_selection(i64::MAX / 2),
            Command::HalfPageDown(n) | Command::ScreenDown(n) => {
                self.move_selection(10 * *n as i64)
            }
            Command::HalfPageUp(n) | Command::ScreenUp(n) => self.move_selection(-10 * *n as i64),
            _ => return false,
        }
        true
    }

    fn bind(&mut self, seq: KeySeq, cmd: Command) {
        self.keymap_mut().bind_parsed(seq.clone(), cmd.clone());
        self.chip = self
            .keymap()
            .keys_for(&cmd)
            .iter()
            .position(|k| *k == seq)
            .unwrap_or(0);
        self.changed = true;
    }

    /// What binding `seq` to `cmd` would do to other bindings, in words.
    fn conflict_text(&self, seq: &[String], cmd: &Command) -> Option<String> {
        let notes: Vec<String> = self
            .keymap()
            .conflicts(seq)
            .into_iter()
            .filter(|(s, c)| !(s.as_slice() == seq && c == cmd))
            .map(|(s, c)| {
                let what = if s.as_slice() == seq {
                    "replaces"
                } else {
                    "stops"
                };
                format!(
                    "{what} {} ({})",
                    seq_to_string(&s),
                    label(&c).to_lowercase()
                )
            })
            .collect();
        (!notes.is_empty()).then(|| notes.join(", "))
    }

    pub fn show(&mut self, ui: &mut egui::Ui, font_size: f32, height: f32) {
        ui.horizontal(|ui| {
            for (mode, name) in [(Mode::Normal, "Normal mode"), (Mode::Visual, "Visual mode")] {
                if ui.selectable_label(self.mode == mode, name).clicked() && self.mode != mode {
                    self.switch_mode();
                }
            }
            ui.add_space(12.0);
            let filter = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter (/)")
                    .desired_width(font_size * 12.0),
            );
            if std::mem::take(&mut self.focus_filter) {
                filter.request_focus();
            }
            if filter.changed() {
                self.selected = 0;
                self.chip = 0;
            }
            if ui
                .button("Reset all")
                .on_hover_text("Every key back to its default")
                .clicked()
            {
                self.reset_all();
            }
        });
        ui.add_space(4.0);
        let rows = self.rows();
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        let label_width = font_size * 13.0;
        let mut action: Option<Action> = None;
        ScrollArea::vertical()
            .id_salt("vimpdf-keys")
            .max_height(height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, cmd) in rows.iter().enumerate() {
                    let picked = i == self.selected;
                    let keys = self.keymap().keys_for(cmd);
                    let default = self.defaults().keys_for(cmd);
                    let recording = self
                        .recording
                        .as_ref()
                        .filter(|(c, _)| c == cmd)
                        .map(|(_, s)| s.clone());
                    let fill = if picked {
                        egui::Color32::from_gray(52)
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let response = Frame::NONE
                        .fill(fill)
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .corner_radius(3.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(label_width, font_size * 1.4),
                                    Sense::hover(),
                                );
                                ui.painter().text(
                                    rect.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    label(cmd),
                                    egui::FontId::proportional(font_size.min(18.0)),
                                    ui.visuals().text_color(),
                                );
                                ui.interact(
                                    rect,
                                    egui::Id::unique(("vimpdf-keys-label", i)),
                                    Sense::hover(),
                                )
                                .on_hover_text(cmd.describe());
                                for (k, seq) in keys.iter().enumerate() {
                                    let text = format!("{}  ×", seq_to_string(seq));
                                    let mut button =
                                        egui::Button::new(RichText::new(text).monospace()).small();
                                    if picked && k == self.chip && recording.is_none() {
                                        button = button.stroke(Stroke::new(1.5, ACCENT));
                                    }
                                    let removed = ui
                                        .add(button)
                                        .on_hover_text(format!("Remove {}", seq_to_string(seq)));
                                    if removed.clicked() {
                                        let seq = seq.clone();
                                        action = Some(Box::new(move |p: &mut Self| p.unbind(&seq)));
                                    }
                                }
                                match &recording {
                                    Some(seq) => {
                                        let shown = if seq.is_empty() {
                                            "press keys…".to_owned()
                                        } else {
                                            seq_to_string(seq)
                                        };
                                        ui.label(RichText::new(shown).monospace().color(ACCENT));
                                        if ui.small_button("Done").clicked() && !seq.is_empty() {
                                            let (seq, cmd) = (seq.clone(), cmd.clone());
                                            action = Some(Box::new(move |p: &mut Self| {
                                                p.recording = None;
                                                p.bind(seq, cmd);
                                            }));
                                        }
                                        if ui.small_button("Cancel").clicked() {
                                            action =
                                                Some(Box::new(|p: &mut Self| p.recording = None));
                                        }
                                    }
                                    None => {
                                        let add = ui
                                            .small_button("+")
                                            .on_hover_text("Add a key: press it, then Enter (a)");
                                        if add.clicked() {
                                            let cmd = cmd.clone();
                                            action = Some(Box::new(move |p: &mut Self| {
                                                p.recording = Some((cmd, Vec::new()));
                                            }));
                                        }
                                    }
                                }
                                if keys != default {
                                    let reset =
                                        ui.small_button("↺").on_hover_text(if default.is_empty() {
                                            "Back to the default (no key)".to_owned()
                                        } else {
                                            let d: Vec<String> =
                                                default.iter().map(|s| seq_to_string(s)).collect();
                                            format!("Back to the default: {}", d.join("  "))
                                        });
                                    if reset.clicked() {
                                        let cmd = cmd.clone();
                                        action = Some(Box::new(move |p: &mut Self| p.reset(&cmd)));
                                    }
                                }
                            });
                            if let Some(seq) = &recording {
                                let note = if seq.is_empty() {
                                    "Press the key or keys (up to 4), then Enter; Esc cancels."
                                        .to_owned()
                                } else {
                                    match self.conflict_text(seq, cmd) {
                                        Some(c) => format!("Enter: bind it; {c}."),
                                        None => "Enter: bind it.".to_owned(),
                                    }
                                };
                                let color = if note.contains("replaces") || note.contains("stops") {
                                    ERROR
                                } else {
                                    BAR_DIM
                                };
                                ui.label(RichText::new(note).small().color(color));
                            }
                        })
                        .response
                        .interact(Sense::click());
                    if response.clicked() && !picked {
                        action = Some(Box::new(move |p: &mut Self| {
                            p.selected = i;
                            p.chip = 0;
                        }));
                    }
                    if picked && self.follow {
                        response.scroll_to_me(Some(Align::Center));
                    }
                }
                if rows.is_empty() {
                    ui.label(RichText::new("No command matches.").color(BAR_DIM));
                }
            });
        self.follow = false;
        if let Some(action) = action {
            action(self);
        }
        // While recording, keys must not press a focused button.
        if self.recording.is_some()
            && let Some(id) = ui.memory(|m| m.focused())
        {
            ui.memory_mut(|m| m.surrender_focus(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::keymap::parse_sequence;

    fn page() -> KeysPage {
        KeysPage::new(Keymap::default_normal(), Keymap::default_visual())
    }

    fn select(p: &mut KeysPage, name: &str) {
        let cmd = Command::parse(name).unwrap();
        p.selected = p.rows().iter().position(|c| *c == cmd).unwrap();
    }

    fn keys(p: &KeysPage, name: &str) -> Vec<String> {
        p.keymap()
            .keys_for(&Command::parse(name).unwrap())
            .iter()
            .map(|s| seq_to_string(s))
            .collect()
    }

    #[test]
    fn record_a_key_then_remove_and_reset() {
        let mut p = page();
        select(&mut p, "quit");
        for tok in ["a", "<C-w>", "<CR>"] {
            assert!(p.key(tok));
        }
        assert!(keys(&p, "quit").contains(&"<C-w>".to_owned()));
        assert!(p.take_changed());
        assert!(
            p.overrides()
                .normal
                .contains(&("<C-w>".into(), "quit".into()))
        );
        // Pick "q" and remove it.
        p.chip = keys(&p, "quit").iter().position(|k| k == "q").unwrap();
        p.key("x");
        assert!(!keys(&p, "quit").contains(&"q".to_owned()));
        p.key("r");
        assert_eq!(keys(&p, "quit"), page().keys_for_test("quit"));
        assert!(p.overrides().normal.is_empty());
    }

    impl KeysPage {
        fn keys_for_test(&self, name: &str) -> Vec<String> {
            keys(self, name)
        }
    }

    #[test]
    fn recording_takes_sequences_and_esc_cancels() {
        let mut p = page();
        select(&mut p, "fit_width");
        for tok in ["a", "z", "w"] {
            p.key(tok);
        }
        assert!(p.recording());
        p.key("<Esc>");
        assert!(!p.recording());
        assert!(p.take_esc_used());
        assert!(!keys(&p, "fit_width").contains(&"zw".to_owned()));
        for tok in ["<CR>", "z", "w", "<CR>"] {
            p.key(tok);
        }
        assert!(keys(&p, "fit_width").contains(&"zw".to_owned()));
        let seq = parse_sequence("zw").unwrap();
        assert_eq!(p.keymap().get(&seq), Some(&Command::FitWidth));
    }

    #[test]
    fn conflicts_are_explained() {
        let p = page();
        let j = parse_sequence("j").unwrap();
        let text = p.conflict_text(&j, &Command::Quit).unwrap();
        assert!(text.starts_with("replaces j (scroll down)"), "{text}");
        assert!(p.conflict_text(&j, &Command::ScrollDown(1)).is_none());
    }

    #[test]
    fn modes_list_their_own_commands() {
        let mut p = page();
        let normal = p.rows();
        assert!(normal.contains(&Command::Quit));
        assert!(!normal.contains(&Command::SwapEnds));
        p.key("v");
        let visual = p.rows();
        assert!(visual.contains(&Command::SwapEnds));
        assert!(visual.iter().any(|c| matches!(c, Command::Move(..))));
        p.filter = "word".into();
        assert!(
            p.rows()
                .iter()
                .all(|c| label(c).to_lowercase().contains("word"))
        );
    }
}
