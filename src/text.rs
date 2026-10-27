//! A page's text as plain data: characters with boxes, grouped into lines and
//! blocks in MuPDF's reading order. Used by search, and by text selection.

use std::ops::Range;

/// One character and its box, in points from the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextChar {
    pub c: char,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Font size and baseline: more reliable than the box for layout,
    /// since glyph boxes vary a lot between fonts.
    pub size: f32,
    pub baseline: f32,
}

/// An axis-aligned rectangle in page coordinates (points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl TextRect {
    fn union(self, o: TextRect) -> TextRect {
        TextRect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageText {
    pub chars: Vec<TextChar>,
    /// Character index ranges, one per line.
    pub lines: Vec<Range<usize>>,
    /// Line index ranges, one per block (≈ paragraph).
    pub blocks: Vec<Range<usize>>,
}

/// Character index range `start..=end` of a match.
pub type Span = (usize, usize);

/// MuPDF's structure: blocks of lines of characters.
pub type RawBlocks = Vec<Vec<Vec<TextChar>>>;

impl PageText {
    /// Build from MuPDF's blocks, fixing two things that make its raw
    /// structure awkward for reading:
    /// - fragments of one visual line (justified text, sub/superscripts)
    ///   come as separate lines: they are merged, with a space inserted
    ///   where there is a visible gap;
    /// - margin line numbers (draft/review copies) are dropped;
    /// - one paragraph can be spread over several blocks: consecutive blocks
    ///   with the same left edge and a normal line gap are joined, as are a
    ///   block ending a column and one continuing at the top of the next;
    /// - a block can hold a whole column of paragraphs: it is split at
    ///   indented first lines and at larger vertical gaps.
    pub fn from_raw(raw: RawBlocks) -> Self {
        let mut out = PageText::default();
        let raw = drop_line_numbers(raw);
        let blocks = join_blocks(raw.into_iter().map(merge_fragments).collect());
        for lines in blocks {
            for para in split_paragraphs(lines) {
                let first_line = out.lines.len();
                for line in para {
                    let start = out.chars.len();
                    out.chars.extend(line);
                    out.lines.push(start..out.chars.len());
                }
                out.blocks.push(first_line..out.lines.len());
            }
        }
        out
    }

    /// Searchable characters and, for each, the index of the page character
    /// it came from. Whitespace runs become one space, lines are joined with a
    /// space, and a word hyphenated across a line break is joined back
    /// ("stan-" + "dard" → "standard").
    fn searchable(&self, fold: bool) -> (Vec<char>, Vec<usize>) {
        let mut text = Vec::with_capacity(self.chars.len());
        let mut map = Vec::with_capacity(self.chars.len());
        let push_space = |text: &mut Vec<char>, map: &mut Vec<usize>, at: usize| {
            if text.last().is_some_and(|c| *c != ' ') {
                text.push(' ');
                map.push(at);
            }
        };
        for block in &self.blocks {
            for li in block.clone() {
                let line = self.lines[li].clone();
                for i in line.clone() {
                    let c = self.chars[i].c;
                    if c.is_whitespace() {
                        push_space(&mut text, &mut map, i);
                    } else {
                        text.push(if fold { fold_char(c) } else { c });
                        map.push(i);
                    }
                }
                let last_in_block = li + 1 == block.end;
                let hyphenated = !last_in_block
                    && text.last() == Some(&'-')
                    && line.end > line.start
                    && map.last() == Some(&(line.end - 1));
                if hyphenated {
                    text.pop();
                    map.pop();
                } else if let Some(&last) = map.last() {
                    push_space(&mut text, &mut map, last);
                }
            }
        }
        (text, map)
    }

    /// Non-overlapping matches of `needle`, in reading order. Case-insensitive
    /// unless `case_sensitive`; whitespace in the needle matches any run of
    /// whitespace or a line break.
    pub fn find(&self, needle: &str, case_sensitive: bool) -> Vec<Span> {
        let needle: Vec<char> = normalize_query(needle)
            .chars()
            .map(|c| if case_sensitive { c } else { fold_char(c) })
            .collect();
        if needle.is_empty() {
            return Vec::new();
        }
        let (text, map) = self.searchable(!case_sensitive);
        let mut out = Vec::new();
        let mut i = 0;
        while i + needle.len() <= text.len() {
            if text[i..i + needle.len()] == needle[..] {
                out.push((map[i], map[i + needle.len() - 1]));
                i += needle.len();
            } else {
                i += 1;
            }
        }
        out
    }

    /// Rectangles covering characters `start..=end`, one per line.
    pub fn rects(&self, (start, end): Span) -> Vec<TextRect> {
        let mut out = Vec::new();
        for line in &self.lines {
            let from = start.max(line.start);
            let to = end.min(line.end.saturating_sub(1));
            if from > to || line.is_empty() {
                continue;
            }
            let rect = self.chars[from..=to]
                .iter()
                .map(|c| TextRect {
                    x0: c.x0,
                    y0: c.y0,
                    x1: c.x1,
                    y1: c.y1,
                })
                .reduce(TextRect::union);
            out.extend(rect);
        }
        out
    }
}

fn line_box(line: &[TextChar]) -> (f32, f32, f32, f32) {
    line.iter().fold(
        (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ),
        |(x0, y0, x1, y1), c| (x0.min(c.x0), y0.min(c.y0), x1.max(c.x1), y1.max(c.y1)),
    )
}

/// What the layout heuristics need to know about a line.
#[derive(Debug, Clone, Copy)]
struct Metrics {
    x0: f32,
    x1: f32,
    /// Largest font size on the line.
    size: f32,
    /// Baseline of the line's main text (not of sub/superscripts).
    baseline: f32,
}

impl Metrics {
    fn of(line: &[TextChar]) -> Self {
        let (x0, _, x1, _) = line_box(line);
        let size = line.iter().map(|c| c.size).fold(1.0, f32::max);
        let baseline = line
            .iter()
            .find(|c| c.size >= 0.8 * size && !c.c.is_whitespace())
            .or(line.first())
            .map_or(0.0, |c| c.baseline);
        Self {
            x0,
            x1,
            size,
            baseline,
        }
    }

    /// Vertical overlap of the two lines' text bands (baseline - size to
    /// baseline), relative to the smaller size.
    fn same_row(&self, o: &Metrics) -> bool {
        let overlap =
            self.baseline.min(o.baseline) - (self.baseline - self.size).max(o.baseline - o.size);
        overlap > 0.3 * self.size.min(o.size)
    }
}

/// Merge consecutive lines on the same row that continue to the right.
fn merge_fragments(lines: Vec<Vec<TextChar>>) -> Vec<Vec<TextChar>> {
    let mut out: Vec<Vec<TextChar>> = Vec::new();
    for line in lines.into_iter().filter(|l| !l.is_empty()) {
        if let Some(prev) = out.last_mut() {
            let p = Metrics::of(prev);
            let l = Metrics::of(&line);
            // A continuation starts to the right; stacked sub/superscripts
            // can start a little before the end of the line, but a wrapped
            // line starts back at the left.
            let continues = l.x0 >= p.x1 - 1.0 || l.x0 > p.x0 + 0.5 * (p.x1 - p.x0);
            if p.same_row(&l) && continues {
                let gap = l.x0 - p.x1;
                let ends_with_space = prev.last().is_some_and(|c| c.c.is_whitespace());
                let starts_with_space = line[0].c.is_whitespace();
                if gap > 0.15 * p.size && !ends_with_space && !starts_with_space {
                    let (_, y0, _, y1) = line_box(prev);
                    prev.push(TextChar {
                        c: ' ',
                        x0: p.x1,
                        y0,
                        x1: l.x0,
                        y1,
                        size: p.size,
                        baseline: p.baseline,
                    });
                }
                prev.extend(line);
                continue;
            }
        }
        out.push(line);
    }
    out
}

/// Drop lines made only of digits that sit in the margin beside a text line
/// on the same row: line numbers of draft and review copies.
fn drop_line_numbers(raw: RawBlocks) -> RawBlocks {
    let text_lines: Vec<Metrics> = raw
        .iter()
        .flatten()
        .filter(|l| !is_number(l))
        .map(|l| Metrics::of(l))
        .collect();
    let is_margin_number = |line: &[TextChar]| {
        if !is_number(line) {
            return false;
        }
        let m = Metrics::of(line);
        text_lines.iter().any(|t| {
            let beside = m.x1 < t.x0 - 0.3 * m.size || m.x0 > t.x1 + 0.3 * m.size;
            m.same_row(t) && beside
        })
    };
    raw.into_iter()
        .map(|block| {
            block
                .into_iter()
                .filter(|l| !is_margin_number(l))
                .collect::<Vec<_>>()
        })
        .filter(|b| !b.is_empty())
        .collect()
}

fn is_number(line: &[TextChar]) -> bool {
    let digits = line.iter().filter(|c| c.c.is_ascii_digit()).count();
    digits > 0
        && digits <= 5
        && line
            .iter()
            .all(|c| c.c.is_ascii_digit() || c.c.is_whitespace())
}

/// Baseline-to-baseline distance (in font sizes) above which a new
/// paragraph starts. Normal leading is about 1.2–1.4.
const PARAGRAPH_GAP: f32 = 1.6;
/// Indentation (in font sizes) that marks a paragraph's first line.
const INDENT: f32 = 0.6;

/// Join consecutive blocks that continue each other: same left edge and a
/// normal line distance below, or (a paragraph running into the next
/// column) starting higher up to the right after a full-width last line.
fn join_blocks(blocks: Vec<Vec<Vec<TextChar>>>) -> Vec<Vec<Vec<TextChar>>> {
    let span = |lines: &[Vec<TextChar>]| {
        lines
            .iter()
            .map(|l| Metrics::of(l))
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), m| {
                (a.min(m.x0), b.max(m.x1))
            })
    };
    let mut out: Vec<Vec<Vec<TextChar>>> = Vec::new();
    for block in blocks.into_iter().filter(|b| !b.is_empty()) {
        if let Some(prev) = out.last_mut() {
            let (prev_left, prev_right) = span(prev);
            let (left, _) = span(&block);
            let last = Metrics::of(prev.last().expect("non-empty"));
            let first = Metrics::of(&block[0]);
            let size = last.size.max(first.size);
            let pitch = first.baseline - last.baseline;
            let aligned = (prev_left - left).abs() < INDENT * size;
            let below = aligned && pitch > 0.3 * size && pitch < PARAGRAPH_GAP * size;
            let next_column = pitch < 0.0
                && left > prev_right
                && last.x1 > prev_right - 2.0 * size
                && first.x0 < left + INDENT * size
                && prev.len() > 1;
            if below || next_column {
                prev.extend(block);
                continue;
            }
        }
        out.push(block);
    }
    out
}

/// Split lines into paragraphs at indented first lines and larger gaps.
fn split_paragraphs(lines: Vec<Vec<TextChar>>) -> Vec<Vec<Vec<TextChar>>> {
    let metrics: Vec<Metrics> = lines.iter().map(|l| Metrics::of(l)).collect();
    // Left edge of the column a line is in: the leftmost line overlapping it
    // horizontally (a block may span two columns after `join_blocks`).
    let column_left = |i: usize| {
        let m = metrics[i];
        metrics
            .iter()
            .filter(|o| o.x0 < m.x1 && o.x1 > m.x0)
            .map(|o| o.x0)
            .fold(f32::INFINITY, f32::min)
    };
    let indented = |i: usize| metrics[i].x0 > column_left(i) + INDENT * metrics[i].size;
    let mut paras: Vec<Vec<Vec<TextChar>>> = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        let new_para = i == 0 || {
            let (prev, cur) = (metrics[i - 1], metrics[i]);
            let gap = cur.baseline - prev.baseline > PARAGRAPH_GAP * prev.size.max(cur.size);
            // An indented line right after another indented one is more
            // likely a centred or list block than a new paragraph.
            (indented(i) && !indented(i - 1)) || gap
        };
        if new_para {
            paras.push(Vec::new());
        }
        paras.last_mut().expect("pushed above").push(line);
    }
    paras
}

fn fold_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Collapse whitespace runs and trim.
pub fn normalize_query(q: &str) -> String {
    q.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Smart case: case-sensitive only if the query contains an uppercase letter.
pub fn is_case_sensitive(query: &str) -> bool {
    query.chars().any(char::is_uppercase)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Build a page from lines of text (one block per `\n\n`), each character
    /// 10pt wide and each line 20pt tall.
    pub fn page(text: &str) -> PageText {
        let mut p = PageText::default();
        let mut y = 0.0;
        for block in text.split("\n\n") {
            let first_line = p.lines.len();
            for line in block.lines() {
                let start = p.chars.len();
                for (i, c) in line.chars().enumerate() {
                    let x = i as f32 * 10.0;
                    p.chars.push(TextChar {
                        c,
                        x0: x,
                        y0: y,
                        x1: x + 10.0,
                        y1: y + 15.0,
                        size: 15.0,
                        baseline: y + 12.0,
                    });
                }
                p.lines.push(start..p.chars.len());
                y += 20.0;
            }
            p.blocks.push(first_line..p.lines.len());
        }
        p
    }

    fn found(p: &PageText, needle: &str) -> Vec<String> {
        p.find(needle, is_case_sensitive(needle))
            .into_iter()
            .map(|(s, e)| p.chars[s..=e].iter().map(|c| c.c).collect())
            .collect()
    }

    #[test]
    fn smart_case() {
        let p = page("The model and the Model");
        assert_eq!(found(&p, "model"), vec!["model", "Model"]);
        assert_eq!(found(&p, "Model"), vec!["Model"]);
        assert_eq!(found(&p, "the"), vec!["The", "the"]);
    }

    #[test]
    fn matches_across_lines_and_hyphenation() {
        let p = page("Beyond the Standard\nModel search\nin multi-\nleptonic final");
        assert_eq!(found(&p, "standard model"), vec!["StandardModel"]);
        assert_eq!(found(&p, "multileptonic"), vec!["multi-leptonic"]);
        assert_eq!(found(&p, "standard   model"), vec!["StandardModel"]);
    }

    #[test]
    fn does_not_join_across_blocks() {
        let p = page("ends with multi-\n\nleptonic starts");
        assert!(found(&p, "multileptonic").is_empty());
        assert_eq!(found(&p, "multi- leptonic"), vec!["multi-leptonic"]);
    }

    #[test]
    fn rects_cover_each_line_of_a_match() {
        let p = page("aaa bbb\nccc ddd");
        let m = p.find("bbb ccc", false)[0];
        let r = p.rects(m);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].x0, r[0].x1, r[0].y0), (40.0, 70.0, 0.0));
        assert_eq!((r[1].x0, r[1].x1, r[1].y0), (0.0, 30.0, 20.0));
    }

    fn ch(c: char, x: f32, y: f32) -> TextChar {
        TextChar {
            c,
            x0: x,
            y0: y,
            x1: x + 5.0,
            y1: y + 10.0,
            size: 10.0,
            baseline: y + 8.0,
        }
    }

    fn line(s: &str, x: f32, y: f32) -> Vec<TextChar> {
        s.chars()
            .enumerate()
            .map(|(i, c)| ch(c, x + i as f32 * 5.0, y))
            .collect()
    }

    fn text_of(p: &PageText, line: usize) -> String {
        p.chars[p.lines[line].clone()].iter().map(|c| c.c).collect()
    }

    #[test]
    fn from_raw_merges_fragments_of_one_visual_line() {
        // "gravitational-wave amplitudes" split by justification, then a
        // subscript slightly lower, then the next line.
        let raw = vec![vec![
            line("waves", 0.0, 0.0),
            line("amplitudes", 40.0, 0.0),
            line("x2", 95.0, 4.0),
            line("next line", 0.0, 12.0),
        ]];
        let p = PageText::from_raw(raw);
        assert_eq!(p.lines.len(), 2);
        assert_eq!(text_of(&p, 0), "waves amplitudes x2");
        assert_eq!(text_of(&p, 1), "next line");
    }

    #[test]
    fn from_raw_merges_stacked_scripts_and_split_blocks() {
        // "distance of 410" with a stacked +160/-180 starting before the end
        // of the line, then a paragraph continued in a second block.
        let raw = vec![
            vec![
                line("distance of 410+160", 10.0, 0.0),
                line("-180 Mpc", 80.0, 3.0),
                line("next line", 0.0, 12.0),
            ],
            vec![line("same para", 0.0, 24.0), line("Indented", 10.0, 36.0)],
        ];
        let p = PageText::from_raw(raw);
        assert_eq!(text_of(&p, 0), "distance of 410+160-180 Mpc");
        assert_eq!(p.blocks, vec![0..3, 3..4]);
    }

    #[test]
    fn from_raw_drops_margin_line_numbers() {
        let raw = vec![
            vec![line("Some text here", 50.0, 0.0), line("120", 30.0, 1.0)],
            vec![line("more text", 50.0, 12.0), line("121", 30.0, 12.0)],
            vec![line("Table 3", 50.0, 24.0)],
            vec![line("42", 50.0, 48.0)], // a number on its own: kept
        ];
        let p = PageText::from_raw(raw);
        let all: Vec<String> = (0..p.lines.len()).map(|i| text_of(&p, i)).collect();
        assert_eq!(all, vec!["Some text here", "more text", "Table 3", "42"]);
        assert_eq!(p.blocks, vec![0..3, 3..4]);
    }

    #[test]
    fn from_raw_joins_a_paragraph_continued_in_the_next_column() {
        let raw = vec![
            vec![
                line("Last para of col one, full", 0.0, 100.0),
                line("width line ends here ...", 0.0, 112.0),
            ],
            vec![
                line("continues at the top", 200.0, 0.0),
                line("of col two", 200.0, 12.0),
            ],
            vec![line("New indented para", 210.0, 24.0)],
        ];
        let p = PageText::from_raw(raw);
        assert_eq!(p.blocks, vec![0..4, 4..5]);
    }

    #[test]
    fn from_raw_splits_paragraphs_at_indents_and_gaps() {
        let raw = vec![vec![
            line("First para", 10.0, 0.0),
            line("continues", 0.0, 12.0),
            line("Second para", 10.0, 24.0),
            line("continues", 0.0, 36.0),
            line("After a gap", 0.0, 60.0),
        ]];
        let p = PageText::from_raw(raw);
        assert_eq!(p.blocks, vec![0..2, 2..4, 4..5]);
    }

    #[test]
    fn empty_needle_finds_nothing() {
        assert!(page("abc").find("  ", false).is_empty());
    }
}
