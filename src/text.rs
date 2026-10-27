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

impl PageText {
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

    #[test]
    fn empty_needle_finds_nothing() {
        assert!(page("abc").find("  ", false).is_empty());
    }
}
