//! Hint mode (`f`, `yf`, `gv`): labels on links or words; typing one acts on it.

use super::*;
use crate::selection::{Selection, TextPos};

/// What a hint label points at.
#[derive(Clone)]
enum Target {
    Link(LinkTarget),
    /// A word: its first character.
    Word(TextPos),
}

#[derive(Clone, Copy, PartialEq)]
enum Purpose {
    Link(HintAction),
    /// Start visual mode at the chosen word.
    Word,
}

struct Hint {
    label: String,
    page: usize,
    /// Where the label goes, in page coordinates.
    x: f32,
    y: f32,
    target: Target,
}

/// Hints on screen: labels, and what has been typed so far.
pub(super) struct HintState {
    purpose: Purpose,
    typed: String,
    hints: Vec<Hint>,
}

/// (page, x, y, target) of every candidate, in reading order.
type Candidates = Vec<(usize, f32, f32, Target)>;

impl VimpdfApp {
    /// Label every link on screen (`f`, `yf`).
    pub(super) fn start_hints(&mut self, action: HintAction) {
        let Some(vp) = &self.viewport else { return };
        let layout = &self.view.layout;
        let mut found: Candidates = Vec::new();
        for page in layout.pages_in(vp.y, vp.y + vp.doc_height()) {
            let r = layout.page(page);
            for link in self.view.links.get(page).into_iter().flatten() {
                if in_view(
                    vp,
                    r.x + link.x0,
                    r.y + link.y0,
                    r.x + link.x1,
                    r.y + link.y1,
                ) {
                    found.push((page, link.x0, link.y0, Target::Link(link.target.clone())));
                }
            }
        }
        if found.is_empty() {
            return self.info("No links on screen");
        }
        self.show_hints(Purpose::Link(action), found);
    }

    /// Label every word on screen (`gv`); choosing one starts visual mode.
    pub(super) fn start_word_hints(&mut self) {
        let Some(vp) = &self.viewport else { return };
        let layout = &self.view.layout;
        let mut found: Candidates = Vec::new();
        let mut missing_text = false;
        for page in layout.pages_in(vp.y, vp.y + vp.doc_height()) {
            let r = layout.page(page);
            let Some(text) = self.view.text(page) else {
                missing_text = true;
                continue;
            };
            for line in &text.lines {
                let mut i = line.start;
                while i < line.end {
                    if text.chars[i].c.is_whitespace() {
                        i += 1;
                        continue;
                    }
                    let c = text.chars[i];
                    if in_view(vp, r.x + c.x0, r.y + c.y0, r.x + c.x1, r.y + c.y1) {
                        found.push((page, c.x0, c.y0, Target::Word(TextPos { page, idx: i })));
                    }
                    while i < line.end && !text.chars[i].c.is_whitespace() {
                        i += 1;
                    }
                }
            }
        }
        if found.is_empty() {
            return if missing_text {
                self.info("Text still loading, try again")
            } else {
                self.error("No text on screen")
            };
        }
        self.show_hints(Purpose::Word, found);
    }

    fn show_hints(&mut self, purpose: Purpose, mut found: Candidates) {
        // Reading order: page, then top to bottom, then left to right.
        found.sort_by_key(|(page, x, y, _)| (*page, y.round() as i32, x.round() as i32));
        let labels = links::hint_labels(found.len());
        self.hints = Some(HintState {
            purpose,
            typed: String::new(),
            hints: labels
                .into_iter()
                .zip(found)
                .map(|(label, (page, x, y, target))| Hint {
                    label,
                    page,
                    x,
                    y,
                    target,
                })
                .collect(),
        });
        self.keys.reset();
        self.mode = Mode::Hints;
    }

    /// A key typed in hint mode. Returns a command once a label is complete.
    pub(super) fn hint_key(&mut self, ctx: &egui::Context, tok: &str) -> Option<Command> {
        let state = self.hints.as_mut()?;
        match tok {
            "<Esc>" => {}
            "<BS>" => {
                state.typed.pop();
                return None;
            }
            t if t.chars().count() == 1 => {
                state.typed.push_str(&t.to_lowercase());
                let matching: Vec<&Hint> = state
                    .hints
                    .iter()
                    .filter(|h| h.label.starts_with(&state.typed))
                    .collect();
                match matching.as_slice() {
                    [] => {}
                    [hint] if hint.label == state.typed => {
                        let (purpose, target) = (state.purpose, hint.target.clone());
                        self.hints = None;
                        self.mode = Mode::Normal;
                        return self.act_on_hint(ctx, purpose, target);
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

    fn act_on_hint(
        &mut self,
        ctx: &egui::Context,
        purpose: Purpose,
        target: Target,
    ) -> Option<Command> {
        match (purpose, target) {
            (Purpose::Link(HintAction::Follow), Target::Link(t)) => Some(Command::FollowLink(t)),
            (Purpose::Link(HintAction::CopyAddress), Target::Link(t)) => {
                let text = match &t {
                    LinkTarget::External(uri) => uri.clone(),
                    LinkTarget::Internal { page, .. } => format!("page {}", page + 1),
                };
                ctx.copy_text(text.clone());
                self.info(format!("Copied {text}"));
                None
            }
            (_, Target::Word(pos)) => {
                let x = self.view.doc_text().rect_at(pos).map_or(0.0, |r| r.x0);
                self.selection = Some(Selection::at(pos, x));
                self.mode = Mode::Visual;
                None
            }
            _ => None,
        }
    }

    pub(super) fn draw_hints(&self, painter: &egui::Painter, area: Rect, vp: &Viewport) {
        let Some(state) = &self.hints else { return };
        let font = FontId::monospace(self.config.font_size.max(11.0) - 1.0);
        let bg = color32(self.config.colors.hint_background);
        let layout = &self.view.layout;
        for hint in &state.hints {
            if !hint.label.starts_with(&state.typed) {
                continue;
            }
            let r = layout.page(hint.page);
            let (x, y) = vp.to_view(r.x + hint.x, r.y + hint.y);
            let pos = area.min + Vec2::new(x, y);
            let galley =
                painter.layout_no_wrap(hint.label.to_uppercase(), font.clone(), Color32::BLACK);
            let rect = Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(3.0, 1.0));
            painter.rect_filled(rect, 3.0, bg);
            painter.galley(pos, galley, Color32::BLACK);
        }
    }
}

fn in_view(vp: &Viewport, x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
    x1 >= vp.x && x0 <= vp.x + vp.doc_width() && y1 >= vp.y && y0 <= vp.y + vp.doc_height()
}
