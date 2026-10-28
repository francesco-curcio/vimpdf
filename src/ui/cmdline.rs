use eframe::egui::{
    self, Align, Align2, FontId, Frame, Id, Key, Layout, Rect, RichText, TextEdit, UiBuilder, Vec2,
};

use super::{ACCENT, BAR_BACKGROUND, BAR_DIM, BAR_TEXT};
use crate::input::complete::{Completion, common_prefix};

const HISTORY_LIMIT: usize = 200;
/// Matches listed above the line at most.
const MAX_LISTED: usize = 120;

/// Completes the line's text (Tab).
pub type Completer<'a> = &'a dyn Fn(&str) -> Completion;

/// Tab completion in progress: the matches and which one the line shows.
struct Cycle {
    start: usize,
    candidates: Vec<String>,
    index: Option<usize>,
    /// The line as completion left it: any other edit ends the cycle.
    shown: String,
}

/// A one-line prompt (`:` command line or `/` search): text being typed
/// and history.
pub struct CmdLine {
    prompt: char,
    pub text: String,
    history: Vec<String>,
    /// Position while browsing history with Up/Down.
    browsing: Option<usize>,
    needs_focus: bool,
    cycle: Option<Cycle>,
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
            cycle: None,
        }
    }

    pub fn open(&mut self, prefill: &str) {
        self.text = prefill.to_owned();
        self.browsing = None;
        self.needs_focus = true;
        self.cycle = None;
    }

    /// Tab (`back`: Shift+Tab). The first Tab completes as far as all
    /// matches agree and lists them; the next ones step through them.
    fn tab(&mut self, complete: Completer<'_>, back: bool) {
        if let Some(c) = self.cycle.as_mut()
            && c.candidates.len() > 1
        {
            let n = c.candidates.len();
            let i = match (c.index, back) {
                (None, false) => 0,
                (None, true) => n - 1,
                (Some(i), false) => (i + 1) % n,
                (Some(i), true) => (i + n - 1) % n,
            };
            c.index = Some(i);
            self.text = format!("{}{}", &self.text[..c.start], c.candidates[i]);
            c.shown = self.text.clone();
            return;
        }
        let Completion { start, candidates } = complete(&self.text);
        self.cycle = None;
        match candidates.as_slice() {
            [] => {}
            [only] => self.text = format!("{}{only}", &self.text[..start]),
            _ => {
                let common = common_prefix(&candidates);
                if common.len() > self.text.len() - start {
                    self.text = format!("{}{common}", &self.text[..start]);
                }
                self.cycle = Some(Cycle {
                    start,
                    candidates,
                    index: None,
                    shown: self.text.clone(),
                });
            }
        }
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

    /// Draw the line in `rect`. With a `completer`, Tab completes.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        font_size: f32,
        completer: Option<Completer<'_>>,
    ) -> Outcome {
        ui.painter().rect_filled(rect, 0.0, BAR_BACKGROUND);
        let font = FontId::monospace(font_size);
        let id = Id::unique(("vimpdf-cmdline", self.prompt));
        let was_empty = self.text.is_empty();
        let (enter, escape, backspace, up, down, tab, shift) = ui.input(|i| {
            (
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Backspace),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Tab),
                i.modifiers.shift,
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
                .desired_width(f32::INFINITY)
                // Tab completes instead of moving the focus away (a single
                // line never inserts a tab character).
                .lock_focus(true),
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
        if self.cycle.as_ref().is_some_and(|c| c.shown != self.text) {
            self.cycle = None;
        }
        if tab
            && response.has_focus()
            && let Some(complete) = completer
        {
            self.tab(complete, shift);
            move_cursor_to_end(ui.ctx(), id, &self.text);
        }
        if let Some(c) = &self.cycle {
            show_matches(ui.ctx(), rect, font_size, c);
        }

        if enter {
            self.cycle = None;
            let line = std::mem::take(&mut self.text);
            self.push_history(&line);
            return Outcome::Submit(line);
        }
        // Esc, Backspace on an empty line (like vim), or clicking away.
        if escape || (backspace && was_empty) || response.lost_focus() {
            self.cycle = None;
            self.text.clear();
            return Outcome::Cancel;
        }
        Outcome::Editing
    }
}

/// The completion matches, above the line; the one shown is highlighted.
/// Paths show only their last part.
fn show_matches(ctx: &egui::Context, rect: Rect, font_size: f32, c: &Cycle) {
    let font = FontId::monospace(font_size);
    egui::Area::new(Id::unique("vimpdf-cmdline-matches"))
        .order(egui::Order::Foreground)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(rect.left_top())
        .interactable(false)
        .show(ctx, |ui| {
            Frame::NONE
                .fill(BAR_BACKGROUND)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_width(rect.width() - 16.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = font_size;
                        for (i, cand) in c.candidates.iter().enumerate().take(MAX_LISTED) {
                            let trimmed = cand.trim_end_matches('/');
                            let name = &cand[trimmed.rfind('/').map_or(0, |i| i + 1)..];
                            let text = RichText::new(name).font(font.clone());
                            ui.label(if c.index == Some(i) {
                                text.color(BAR_BACKGROUND).background_color(ACCENT)
                            } else {
                                text.color(BAR_TEXT)
                            });
                        }
                        if c.candidates.len() > MAX_LISTED {
                            ui.label(
                                RichText::new(format!(
                                    "… {} more",
                                    c.candidates.len() - MAX_LISTED
                                ))
                                .font(font.clone())
                                .color(BAR_DIM),
                            );
                        }
                    });
                });
        });
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
    fn tab_completes_common_part_then_cycles() {
        let mut c = CmdLine::new(':');
        let complete = |line: &str| {
            let start = line.find(' ').map_or(0, |i| i + 1);
            let word = &line[start..];
            Completion {
                start,
                candidates: ["papers/a.pdf", "papers/b.pdf"]
                    .iter()
                    .filter(|p| p.starts_with(word))
                    .map(|p| p.to_string())
                    .collect(),
            }
        };
        c.text = "open pa".into();
        c.tab(&complete, false);
        assert_eq!(c.text, "open papers/", "as far as both agree");
        c.tab(&complete, false);
        assert_eq!(c.text, "open papers/a.pdf");
        c.tab(&complete, false);
        assert_eq!(c.text, "open papers/b.pdf");
        c.tab(&complete, false);
        assert_eq!(c.text, "open papers/a.pdf", "wraps around");
        c.tab(&complete, true);
        assert_eq!(c.text, "open papers/b.pdf", "Shift+Tab goes back");

        // A single match completes at once.
        c.cycle = None;
        c.text = "open papers/a".into();
        c.tab(&complete, false);
        assert_eq!(c.text, "open papers/a.pdf");
        assert!(c.cycle.is_none());
        // No match leaves the line alone.
        c.text = "open zz".into();
        c.tab(&complete, false);
        assert_eq!(c.text, "open zz");
    }

    /// One frame of the command line, with `events` as input.
    fn frame(
        ctx: &egui::Context,
        c: &mut CmdLine,
        events: Vec<egui::Event>,
        complete: Completer<'_>,
    ) -> Outcome {
        let mut outcome = Outcome::Editing;
        let input = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800.0, 600.0),
            )),
            ..Default::default()
        };
        // Nothing is painted: drop the font atlas upload.
        ctx.run_ui(input, |ui| {
            let rect = Rect::from_min_size(egui::Pos2::new(0.0, 570.0), Vec2::new(800.0, 30.0));
            outcome = c.show(ui, rect, 14.0, Some(complete));
        })
        .textures_delta
        .clear();
        outcome
    }

    #[test]
    fn a_real_tab_key_completes_and_keeps_the_line_open() {
        let ctx = egui::Context::default();
        let mut c = CmdLine::new(':');
        let complete = |line: &str| Completion {
            start: 0,
            candidates: if "saveas".starts_with(line) {
                vec!["saveas".into()]
            } else {
                Vec::new()
            },
        };
        let tab = egui::Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        c.open("sav");
        frame(&ctx, &mut c, Vec::new(), &complete); // takes the focus
        frame(&ctx, &mut c, Vec::new(), &complete);
        assert!(matches!(
            frame(&ctx, &mut c, vec![tab.clone()], &complete),
            Outcome::Editing
        ));
        assert_eq!(c.text, "saveas");
        // Focus stays: the next frame doesn't cancel.
        assert!(matches!(
            frame(&ctx, &mut c, Vec::new(), &complete),
            Outcome::Editing
        ));
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
