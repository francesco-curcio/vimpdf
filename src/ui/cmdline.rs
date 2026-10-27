use eframe::egui::{
    self, Align, FontId, Frame, Id, Key, Layout, Rect, RichText, TextEdit, UiBuilder, Vec2,
};

use super::{ACCENT, BAR_BACKGROUND, BAR_TEXT};

const HISTORY_LIMIT: usize = 200;

/// A one-line prompt (`:` command line or `/` search): text being typed
/// and history.
pub struct CmdLine {
    prompt: char,
    pub text: String,
    history: Vec<String>,
    /// Position while browsing history with Up/Down.
    browsing: Option<usize>,
    needs_focus: bool,
}

pub enum Outcome {
    Editing,
    Submit(String),
    Cancel,
}

impl CmdLine {
    pub fn new(prompt: char) -> Self {
        Self {
            prompt,
            text: String::new(),
            history: Vec::new(),
            browsing: None,
            needs_focus: false,
        }
    }

    pub fn open(&mut self, prefill: &str) {
        self.text = prefill.to_owned();
        self.browsing = None;
        self.needs_focus = true;
    }

    pub fn push_history(&mut self, line: &str) {
        if !line.trim().is_empty() && self.history.last().map(String::as_str) != Some(line) {
            self.history.push(line.to_owned());
            if self.history.len() > HISTORY_LIMIT {
                self.history.remove(0);
            }
        }
    }

    /// Up (`older`) / Down through history. Returns the new text.
    fn browse(&mut self, older: bool) -> Option<&str> {
        let len = self.history.len();
        let next = match (self.browsing, older) {
            (None, true) if len > 0 => Some(len - 1),
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i + 1 < len => Some(i + 1),
            (Some(_), false) => None,
            (None, _) => return None,
        };
        self.browsing = next;
        Some(next.map_or("", |i| self.history[i].as_str()))
    }

    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, font_size: f32) -> Outcome {
        ui.painter().rect_filled(rect, 0.0, BAR_BACKGROUND);
        let font = FontId::monospace(font_size);
        let id = Id::unique(("vimpdf-cmdline", self.prompt));
        let was_empty = self.text.is_empty();
        let (enter, escape, backspace, up, down) = ui.input(|i| {
            (
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Backspace),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
            )
        });

        let mut line = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(8.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        line.spacing_mut().item_spacing.x = 0.0;
        line.label(
            RichText::new(self.prompt.to_string())
                .font(font.clone())
                .color(ACCENT),
        );
        let response = line.add(
            TextEdit::singleline(&mut self.text)
                .id(id)
                .font(font)
                .text_color(BAR_TEXT)
                .frame(Frame::NONE)
                .desired_width(f32::INFINITY),
        );
        if self.needs_focus {
            // Focus after the widget has been drawn: this frame's keys (the
            // ':' and anything typed with it) were already handled and passed
            // in as the prefill; from the next frame on the field gets them.
            response.request_focus();
            move_cursor_to_end(line.ctx(), id, &self.text);
            self.needs_focus = false;
            return Outcome::Editing;
        }
        if response.has_focus()
            && (up || down)
            && let Some(text) = self.browse(up)
        {
            self.text = text.to_owned();
            move_cursor_to_end(ui.ctx(), id, &self.text);
        }

        if enter {
            let line = std::mem::take(&mut self.text);
            self.push_history(&line);
            return Outcome::Submit(line);
        }
        // Esc, Backspace on an empty line (like vim), or clicking away.
        if escape || (backspace && was_empty) || response.lost_focus() {
            self.text.clear();
            return Outcome::Cancel;
        }
        Outcome::Editing
    }
}

/// Keys typed after ':' before the command line widget exists.
#[derive(Debug, PartialEq)]
pub enum TypeAhead {
    /// Still typing: open the command line with this text.
    Editing(String),
    /// Enter was pressed.
    Submitted(String),
    /// Esc, or Backspace on an empty line.
    Cancelled,
}

/// Consume key tokens into a command line starting from `text`, stopping at
/// Enter or Esc (later tokens stay in `tokens` for normal mode).
pub fn type_ahead(tokens: &mut impl Iterator<Item = String>, mut text: String) -> TypeAhead {
    for tok in tokens.by_ref() {
        match tok.as_str() {
            "<CR>" => return TypeAhead::Submitted(text),
            "<Esc>" => return TypeAhead::Cancelled,
            "<BS>" if text.is_empty() => return TypeAhead::Cancelled,
            "<BS>" => {
                text.pop();
            }
            "<Space>" => text.push(' '),
            t if t.chars().count() == 1 => text.push_str(t),
            _ => {}
        }
    }
    TypeAhead::Editing(text)
}

fn move_cursor_to_end(ctx: &egui::Context, id: Id, text: &str) {
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let end = egui::text::CCursor::new(text.chars().count());
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(end)));
    state.store(ctx, id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_ahead_handles_enter_escape_and_backspace() {
        let toks = |s: &[&str]| s.iter().map(|t| t.to_string()).collect::<Vec<_>>();

        let mut it = toks(&["1", "2", "<CR>", "j"]).into_iter();
        assert_eq!(
            type_ahead(&mut it, String::new()),
            TypeAhead::Submitted("12".into())
        );
        assert_eq!(
            it.next().as_deref(),
            Some("j"),
            "later keys stay for normal mode"
        );

        let mut it = toks(&["s", "e", "<Space>", "x", "<BS>", "t"]).into_iter();
        assert_eq!(
            type_ahead(&mut it, String::new()),
            TypeAhead::Editing("se t".into())
        );

        let mut it = toks(&["<BS>"]).into_iter();
        assert_eq!(type_ahead(&mut it, String::new()), TypeAhead::Cancelled);
        let mut it = toks(&["a", "<Esc>"]).into_iter();
        assert_eq!(type_ahead(&mut it, String::new()), TypeAhead::Cancelled);
    }

    #[test]
    fn history_browsing() {
        let mut c = CmdLine::new(':');
        assert_eq!(c.browse(true), None);
        c.push_history("set toolbar");
        c.push_history("12");
        c.push_history("12");
        c.push_history("  ");
        assert_eq!(c.history.len(), 2);

        assert_eq!(c.browse(true), Some("12"));
        assert_eq!(c.browse(true), Some("set toolbar"));
        assert_eq!(c.browse(true), Some("set toolbar"));
        assert_eq!(c.browse(false), Some("12"));
        assert_eq!(c.browse(false), Some(""));
        assert_eq!(c.browse(false), None);
    }
}
