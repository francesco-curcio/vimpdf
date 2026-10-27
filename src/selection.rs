//! Text positions, caret motions, selections and copying, across pages.
//!
//! Everything works on the per-page `PageText` models the UI has loaded;
//! motions stop at pages whose text isn't loaded (yet).

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use crate::text::{PageText, TextRect};

/// A character in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextPos {
    pub page: usize,
    pub idx: usize,
}

impl PartialOrd for TextPos {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TextPos {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.page, self.idx).cmp(&(other.page, other.idx))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Down,
    Up,
    WordForward,
    WordBackward,
    WordEnd,
    LineStart,
    LineEnd,
    BlockForward,
    BlockBackward,
}

impl Motion {
    pub const ALL: &[(&str, Motion)] = &[
        ("caret_left", Motion::Left),
        ("caret_right", Motion::Right),
        ("caret_down", Motion::Down),
        ("caret_up", Motion::Up),
        ("word_forward", Motion::WordForward),
        ("word_backward", Motion::WordBackward),
        ("word_end", Motion::WordEnd),
        ("line_start", Motion::LineStart),
        ("line_end", Motion::LineEnd),
        ("block_forward", Motion::BlockForward),
        ("block_backward", Motion::BlockBackward),
    ];

    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, m)| *m == self)
            .map(|(n, _)| *n)
            .unwrap_or("?")
    }
}

/// The loaded text of a document.
pub struct DocText<'a> {
    pub pages: &'a HashMap<usize, Arc<PageText>>,
    pub page_count: usize,
}

fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

impl DocText<'_> {
    fn page(&self, page: usize) -> Option<&PageText> {
        self.pages.get(&page).map(|t| t.as_ref())
    }

    pub fn char_at(&self, pos: TextPos) -> Option<char> {
        Some(self.page(pos.page)?.chars.get(pos.idx)?.c)
    }

    pub fn rect_at(&self, pos: TextPos) -> Option<TextRect> {
        let c = self.page(pos.page)?.chars.get(pos.idx)?;
        Some(TextRect {
            x0: c.x0,
            y0: c.y0,
            x1: c.x1,
            y1: c.y1,
        })
    }

    #[cfg(test)]
    fn line_of(&self, pos: TextPos) -> Option<usize> {
        line_index(self.page(pos.page)?, pos.idx)
    }

    /// The next character, and whether a line (or page) boundary was crossed.
    fn next(&self, pos: TextPos) -> Option<(TextPos, bool)> {
        let text = self.page(pos.page)?;
        if pos.idx + 1 < text.chars.len() {
            let line = line_index(text, pos.idx)?;
            let crossed = text.lines[line].end == pos.idx + 1;
            return Some((
                TextPos {
                    page: pos.page,
                    idx: pos.idx + 1,
                },
                crossed,
            ));
        }
        // First character of the next page with (loaded) text.
        let mut page = pos.page + 1;
        while page < self.page_count {
            let t = self.page(page)?;
            if !t.chars.is_empty() {
                return Some((TextPos { page, idx: 0 }, true));
            }
            page += 1;
        }
        None
    }

    /// The previous character, and whether `pos` started a line (or page).
    fn prev(&self, pos: TextPos) -> Option<(TextPos, bool)> {
        let text = self.page(pos.page)?;
        if pos.idx > 0 {
            let line = line_index(text, pos.idx)?;
            let crossed = text.lines[line].start == pos.idx;
            return Some((
                TextPos {
                    page: pos.page,
                    idx: pos.idx - 1,
                },
                crossed,
            ));
        }
        let mut page = pos.page;
        while page > 0 {
            page -= 1;
            let t = self.page(page)?;
            if !t.chars.is_empty() {
                return Some((
                    TextPos {
                        page,
                        idx: t.chars.len() - 1,
                    },
                    true,
                ));
            }
        }
        None
    }

    fn class_at(&self, pos: TextPos) -> u8 {
        self.char_at(pos).map_or(0, class)
    }

    /// Apply `motion` `n` times. `want_x` is the caret's preferred horizontal
    /// position for `Up`/`Down` (kept across vertical moves, like vim).
    pub fn apply(&self, pos: TextPos, motion: Motion, n: usize, want_x: f32) -> TextPos {
        (0..n.max(1)).fold(pos, |p, _| self.apply_once(p, motion, want_x).unwrap_or(p))
    }

    fn apply_once(&self, pos: TextPos, motion: Motion, want_x: f32) -> Option<TextPos> {
        match motion {
            Motion::Left => self.prev(pos).map(|(p, _)| p),
            Motion::Right => self.next(pos).map(|(p, _)| p),
            Motion::WordForward => self.word_forward(pos),
            Motion::WordBackward => self.word_backward(pos),
            Motion::WordEnd => self.word_end(pos),
            Motion::LineStart => {
                let text = self.page(pos.page)?;
                let line = line_index(text, pos.idx)?;
                Some(TextPos {
                    page: pos.page,
                    idx: text.lines[line].start,
                })
            }
            Motion::LineEnd => {
                let text = self.page(pos.page)?;
                let line = line_index(text, pos.idx)?;
                Some(TextPos {
                    page: pos.page,
                    idx: text.lines[line].end - 1,
                })
            }
            Motion::Down => self.vertical(pos, true, want_x),
            Motion::Up => self.vertical(pos, false, want_x),
            Motion::BlockForward => self.block(pos, true),
            Motion::BlockBackward => self.block(pos, false),
        }
    }

    fn word_forward(&self, pos: TextPos) -> Option<TextPos> {
        let start = self.class_at(pos);
        let mut seen_space = start == 0;
        let mut p = pos;
        while let Some((np, crossed)) = self.next(p) {
            p = np;
            let c = self.class_at(p);
            if crossed || c == 0 {
                seen_space = true;
            }
            if c != 0 && (seen_space || c != start) {
                return Some(p);
            }
        }
        Some(p)
    }

    fn word_backward(&self, pos: TextPos) -> Option<TextPos> {
        let (mut p, _) = self.prev(pos)?;
        while self.class_at(p) == 0 {
            p = self.prev(p)?.0;
        }
        let cls = self.class_at(p);
        while let Some((pp, crossed)) = self.prev(p) {
            if crossed || self.class_at(pp) != cls {
                break;
            }
            p = pp;
        }
        Some(p)
    }

    fn word_end(&self, pos: TextPos) -> Option<TextPos> {
        let (mut p, _) = self.next(pos)?;
        while self.class_at(p) == 0 {
            p = self.next(p)?.0;
        }
        let cls = self.class_at(p);
        while let Some((np, crossed)) = self.next(p) {
            if crossed || self.class_at(np) != cls {
                break;
            }
            p = np;
        }
        Some(p)
    }

    fn vertical(&self, pos: TextPos, down: bool, want_x: f32) -> Option<TextPos> {
        let text = self.page(pos.page)?;
        let line = line_index(text, pos.idx)?;
        let (page, line) = if down {
            if line + 1 < text.lines.len() {
                (pos.page, line + 1)
            } else {
                let page = (pos.page + 1..self.page_count)
                    .find(|p| self.page(*p).is_some_and(|t| !t.lines.is_empty()))?;
                (page, 0)
            }
        } else if line > 0 {
            (pos.page, line - 1)
        } else {
            let page = (0..pos.page)
                .rev()
                .find(|p| self.page(*p).is_some_and(|t| !t.lines.is_empty()))?;
            (page, self.page(page)?.lines.len() - 1)
        };
        let text = self.page(page)?;
        Some(TextPos {
            page,
            idx: nearest_in_line(text, line, want_x),
        })
    }

    /// `}`: end of the current block, or of the next one if already there.
    /// `{`: start of the current block, or of the previous one.
    fn block(&self, pos: TextPos, forward: bool) -> Option<TextPos> {
        let text = self.page(pos.page)?;
        let line = line_index(text, pos.idx)?;
        let block = text.blocks.iter().position(|b| b.contains(&line))?;
        let first = |t: &PageText, b: usize| t.lines[t.blocks[b].start].start;
        let last = |t: &PageText, b: usize| t.lines[t.blocks[b].end - 1].end - 1;
        if forward {
            if pos.idx < last(text, block) {
                return Some(TextPos {
                    page: pos.page,
                    idx: last(text, block),
                });
            }
            if block + 1 < text.blocks.len() {
                return Some(TextPos {
                    page: pos.page,
                    idx: last(text, block + 1),
                });
            }
            let page = (pos.page + 1..self.page_count)
                .find(|p| self.page(*p).is_some_and(|t| !t.blocks.is_empty()))?;
            return Some(TextPos {
                page,
                idx: last(self.page(page)?, 0),
            });
        }
        if pos.idx > first(text, block) {
            return Some(TextPos {
                page: pos.page,
                idx: first(text, block),
            });
        }
        if block > 0 {
            return Some(TextPos {
                page: pos.page,
                idx: first(text, block - 1),
            });
        }
        let page = (0..pos.page)
            .rev()
            .find(|p| self.page(*p).is_some_and(|t| !t.blocks.is_empty()))?;
        let t = self.page(page)?;
        Some(TextPos {
            page,
            idx: first(t, t.blocks.len() - 1),
        })
    }

    /// The start and end of the word (or run of punctuation) at `pos`.
    pub fn word_at(&self, pos: TextPos) -> (TextPos, TextPos) {
        let cls = self.class_at(pos);
        let mut start = pos;
        while let Some((p, crossed)) = self.prev(start) {
            if crossed || self.class_at(p) != cls {
                break;
            }
            start = p;
        }
        let mut end = pos;
        while let Some((p, crossed)) = self.next(end) {
            if crossed || self.class_at(p) != cls {
                break;
            }
            end = p;
        }
        (start, end)
    }

    /// First and last character of the line at `pos`.
    pub fn line_at(&self, pos: TextPos) -> Option<(TextPos, TextPos)> {
        let text = self.page(pos.page)?;
        let line = &text.lines[line_index(text, pos.idx)?];
        Some((
            TextPos {
                page: pos.page,
                idx: line.start,
            },
            TextPos {
                page: pos.page,
                idx: line.end - 1,
            },
        ))
    }
}

pub fn line_index(text: &PageText, idx: usize) -> Option<usize> {
    let i = text.lines.partition_point(|l| l.end <= idx);
    (i < text.lines.len() && text.lines[i].contains(&idx)).then_some(i)
}

/// The character of `line` whose centre is closest to `x`.
fn nearest_in_line(text: &PageText, line: usize, x: f32) -> usize {
    let range = text.lines[line].clone();
    range
        .clone()
        .min_by(|&a, &b| {
            let da = ((text.chars[a].x0 + text.chars[a].x1) / 2.0 - x).abs();
            let db = ((text.chars[b].x0 + text.chars[b].x1) / 2.0 - x).abs();
            da.total_cmp(&db)
        })
        .unwrap_or(range.start)
}

/// The character nearest to the page point (`x`, `y`): the line whose
/// vertical extent is closest, then the closest character on it. `None` if
/// the page has no text.
pub fn hit_test(text: &PageText, x: f32, y: f32) -> Option<usize> {
    let line = (0..text.lines.len())
        .min_by(|&a, &b| line_distance(text, a, y).total_cmp(&line_distance(text, b, y)))?;
    Some(nearest_in_line(text, line, x))
}

/// Whether (`x`, `y`) is on a character (for the I-beam cursor).
pub fn over_text(text: &PageText, x: f32, y: f32) -> bool {
    text.chars
        .iter()
        .any(|c| x >= c.x0 && x <= c.x1 && y >= c.y0 && y <= c.y1)
}

fn line_distance(text: &PageText, line: usize, y: f32) -> f32 {
    let range = text.lines[line].clone();
    let (y0, y1) = text.chars[range]
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), c| {
            (a.min(c.y0), b.max(c.y1))
        });
    if y < y0 {
        y0 - y
    } else if y > y1 {
        y - y1
    } else {
        0.0
    }
}

/// An anchor and a caret; the selection covers everything between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    pub anchor: TextPos,
    pub caret: TextPos,
    /// Whole lines (`V`).
    pub linewise: bool,
    /// Preferred caret x for vertical moves.
    pub want_x: f32,
}

impl Selection {
    pub fn at(pos: TextPos, want_x: f32) -> Self {
        Self {
            anchor: pos,
            caret: pos,
            linewise: false,
            want_x,
        }
    }

    /// First and last selected character (inclusive), whole lines if linewise.
    pub fn bounds(&self, text: &DocText) -> (TextPos, TextPos) {
        let (a, b) = if self.anchor <= self.caret {
            (self.anchor, self.caret)
        } else {
            (self.caret, self.anchor)
        };
        if !self.linewise {
            return (a, b);
        }
        let start = text.line_at(a).map_or(a, |(s, _)| s);
        let end = text.line_at(b).map_or(b, |(_, e)| e);
        (start, end)
    }

    /// Per page: the selected character range `start..=end`.
    pub fn page_spans(&self, text: &DocText) -> Vec<(usize, usize, usize)> {
        let (start, end) = self.bounds(text);
        (start.page..=end.page)
            .filter_map(|page| {
                let len = text.page(page)?.chars.len();
                if len == 0 {
                    return None;
                }
                let a = if page == start.page { start.idx } else { 0 };
                let b = if page == end.page { end.idx } else { len - 1 };
                (a <= b).then_some((page, a, b.min(len - 1)))
            })
            .collect()
    }

    /// The selected text, cleaned up for pasting (see [`clean_text`]).
    pub fn text(&self, text: &DocText) -> String {
        let pieces: Vec<String> = self
            .page_spans(text)
            .into_iter()
            .filter_map(|(page, a, b)| Some(clean_text(text.page(page)?, a, b)))
            .collect();
        pieces.join("\n")
    }
}

/// Text of characters `a..=b` on one page, ready to paste: lines of a block
/// joined with spaces, a word hyphenated at a line end joined back, blocks
/// separated by newlines, ligatures expanded, whitespace runs collapsed.
/// A line-end hyphen between lowercase letters is treated as soft.
pub fn clean_text(text: &PageText, a: usize, b: usize) -> String {
    let mut out = String::new();
    let mut prev_block: Option<usize> = None;
    for (bi, block) in text.blocks.iter().enumerate() {
        for li in block.clone() {
            let line = &text.lines[li];
            let from = a.max(line.start);
            let to = b.min(line.end - 1);
            if from > to {
                continue;
            }
            let piece: String = text.chars[from..=to].iter().map(|c| c.c).collect();
            let piece = expand_ligatures(&piece);
            let piece = piece.split_whitespace().collect::<Vec<_>>().join(" ");
            if piece.is_empty() {
                continue;
            }
            match prev_block {
                None => {}
                Some(pb) if pb != bi => out.push('\n'),
                Some(_) if out.ends_with('-') => {
                    // "multi-" + "leptonic": a soft hyphen, drop it. Keep real
                    // ones ("TeV-" + "scale", "all-" + "American").
                    let soft = piece.chars().next().is_some_and(char::is_lowercase)
                        && out.chars().rev().nth(1).is_some_and(char::is_lowercase);
                    if soft {
                        out.pop();
                    }
                }
                Some(_) => out.push(' '),
            }
            out.push_str(&piece);
            prev_block = Some(bi);
        }
    }
    out
}

pub fn expand_ligatures(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::page;

    fn doc(pages: &[&str]) -> HashMap<usize, Arc<PageText>> {
        pages
            .iter()
            .enumerate()
            .map(|(i, t)| (i, Arc::new(page(t))))
            .collect()
    }

    fn pos(page: usize, idx: usize) -> TextPos {
        TextPos { page, idx }
    }

    fn ch(d: &DocText, p: TextPos) -> char {
        d.char_at(p).unwrap()
    }

    #[test]
    fn word_motions() {
        let pages = doc(&["one two, three\nfour"]);
        let d = DocText {
            pages: &pages,
            page_count: 1,
        };
        let w = |p, n| d.apply(p, Motion::WordForward, n, 0.0);
        assert_eq!(w(pos(0, 0), 1), pos(0, 4)); // "two"
        assert_eq!(w(pos(0, 4), 1), pos(0, 7)); // ","
        assert_eq!(w(pos(0, 7), 1), pos(0, 9)); // "three"
        assert_eq!(ch(&d, w(pos(0, 9), 1)), 'f', "w crosses the line break");
        assert_eq!(w(pos(0, 0), 3), pos(0, 9));

        let b = |p| d.apply(p, Motion::WordBackward, 1, 0.0);
        assert_eq!(b(pos(0, 14)), pos(0, 9));
        assert_eq!(b(pos(0, 11)), pos(0, 9));
        assert_eq!(b(pos(0, 9)), pos(0, 7));

        let e = |p| d.apply(p, Motion::WordEnd, 1, 0.0);
        assert_eq!(e(pos(0, 0)), pos(0, 2));
        assert_eq!(e(pos(0, 2)), pos(0, 6));
    }

    #[test]
    fn line_and_vertical_motions() {
        let pages = doc(&["abcdef\nxy\nlonger line"]);
        let d = DocText {
            pages: &pages,
            page_count: 1,
        };
        assert_eq!(d.apply(pos(0, 3), Motion::LineStart, 1, 0.0), pos(0, 0));
        assert_eq!(d.apply(pos(0, 3), Motion::LineEnd, 1, 0.0), pos(0, 5));
        // Each char is 10pt wide: want_x 45 is the 5th char.
        let down = d.apply(pos(0, 4), Motion::Down, 1, 45.0);
        assert_eq!(down, pos(0, 7), "short line: clamps to its last char");
        let down2 = d.apply(down, Motion::Down, 1, 45.0);
        assert_eq!(down2, pos(0, 12), "want_x is kept across lines");
        assert_eq!(d.apply(down2, Motion::Up, 2, 45.0), pos(0, 4));
    }

    #[test]
    fn motions_cross_pages_when_loaded() {
        let pages = doc(&["end of one", "start of two"]);
        let d = DocText {
            pages: &pages,
            page_count: 3,
        };
        assert_eq!(d.apply(pos(0, 9), Motion::Right, 1, 0.0), pos(1, 0));
        assert_eq!(d.apply(pos(1, 0), Motion::Left, 1, 0.0), pos(0, 9));
        assert_eq!(d.apply(pos(0, 7), Motion::WordForward, 1, 0.0), pos(1, 0));
        assert_eq!(d.apply(pos(0, 3), Motion::Down, 1, 0.0), pos(1, 0));
        // Page 2 isn't loaded: stay put.
        assert_eq!(d.apply(pos(1, 11), Motion::Right, 1, 0.0), pos(1, 11));
    }

    #[test]
    fn block_motions() {
        let pages = doc(&["para one\nstill one\n\npara two\n\npara three"]);
        let d = DocText {
            pages: &pages,
            page_count: 1,
        };
        let fwd = |p| d.apply(p, Motion::BlockForward, 1, 0.0);
        let back = |p| d.apply(p, Motion::BlockBackward, 1, 0.0);
        let end_one = fwd(pos(0, 3));
        assert_eq!(d.line_of(end_one), Some(1));
        assert_eq!(ch(&d, end_one), 'e', "last char of 'still one'");
        let end_two = fwd(end_one);
        assert_eq!(d.line_of(end_two), Some(2));
        assert_eq!(ch(&d, end_two), 'o');
        assert_eq!(d.apply(pos(0, 3), Motion::BlockForward, 3, 0.0).idx, 34);

        let start_two = back(end_two);
        assert_eq!((d.line_of(start_two), ch(&d, start_two)), (Some(2), 'p'));
        assert_eq!(back(start_two), pos(0, 0));
    }

    #[test]
    fn hit_testing() {
        let p = page("hello world\nsecond line");
        assert_eq!(hit_test(&p, 3.0, 5.0), Some(0));
        assert_eq!(hit_test(&p, 64.0, 5.0), Some(6));
        assert_eq!(hit_test(&p, 64.0, 26.0), Some(11 + 6));
        assert_eq!(
            hit_test(&p, 999.0, 999.0),
            Some(21),
            "far away: nearest char"
        );
        assert!(over_text(&p, 5.0, 5.0));
        assert!(!over_text(&p, 5.0, 17.0), "between lines");
        assert_eq!(hit_test(&PageText::default(), 0.0, 0.0), None);
    }

    #[test]
    fn selection_text_is_cleaned() {
        let pages = doc(&[
            "The multi-\nleptonic ﬁnal\nstate.\n\nNext  para",
            "Page two",
        ]);
        let d = DocText {
            pages: &pages,
            page_count: 2,
        };
        let mut sel = Selection::at(pos(0, 4), 0.0);
        sel.caret = pos(1, 3);
        assert_eq!(sel.text(&d), "multileptonic final state.\nNext para\nPage");

        // Backwards selection, linewise.
        let mut sel = Selection::at(pos(0, 15), 0.0);
        sel.caret = pos(0, 2);
        sel.linewise = true;
        assert_eq!(sel.text(&d), "The multileptonic final");
    }

    #[test]
    fn real_hyphens_are_kept() {
        let p = page("an all-\nAmerican team\nTeV-\nscale");
        assert_eq!(
            clean_text(&p, 0, p.chars.len() - 1),
            "an all-American team TeV-scale"
        );
    }

    #[test]
    fn word_and_line_at() {
        let pages = doc(&["hello, world\nnext"]);
        let d = DocText {
            pages: &pages,
            page_count: 1,
        };
        assert_eq!(d.word_at(pos(0, 2)), (pos(0, 0), pos(0, 4)));
        assert_eq!(d.word_at(pos(0, 9)), (pos(0, 7), pos(0, 11)));
        assert_eq!(d.line_at(pos(0, 13)), Some((pos(0, 12), pos(0, 15))));
    }
}
