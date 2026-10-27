//! Jump history (`<C-o>` / `<C-i>`) and marks (`m<letter>` / `'<letter>`).
//!
//! Positions are stored as page + offset rather than an absolute document
//! coordinate, so they survive a reload that changes page sizes.

use std::collections::HashMap;

/// A reading position: page, distance of the view top from the page top (PDF
/// points), and horizontal scroll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub page: usize,
    pub offset: f32,
    pub x: f32,
}

const LIMIT: usize = 100;

#[derive(Debug, Default)]
pub struct JumpList {
    jumps: Vec<Position>,
    /// Index of the entry we're at while moving through history; `len()`
    /// when not moving through it.
    index: usize,
    marks: HashMap<char, Position>,
}

impl JumpList {
    /// Record `from` before a jump. Forgets anything ahead of the current
    /// point (like a browser's history).
    pub fn push(&mut self, from: Position) {
        self.jumps.truncate(self.index);
        if self.jumps.last() != Some(&from) {
            self.jumps.push(from);
        }
        if self.jumps.len() > LIMIT {
            self.jumps.remove(0);
        }
        self.index = self.jumps.len();
    }

    /// `<C-o>`: go back `n` jumps from `current`.
    pub fn back(&mut self, current: Position, n: usize) -> Option<Position> {
        if self.index == 0 || n == 0 {
            return None;
        }
        if self.index == self.jumps.len() {
            // Remember where we are so <C-i> can return here.
            self.jumps.push(current);
        }
        self.index = self.index.saturating_sub(n);
        Some(self.jumps[self.index])
    }

    /// `<C-i>`: go forward `n` jumps.
    pub fn forward(&mut self, n: usize) -> Option<Position> {
        if n == 0 || self.index + n >= self.jumps.len() {
            return None;
        }
        self.index += n;
        let pos = self.jumps[self.index];
        if self.index == self.jumps.len() - 1 {
            // Back at the newest entry: it was the "current" placeholder.
            self.jumps.pop();
            self.index = self.jumps.len();
        }
        Some(pos)
    }

    pub fn set_mark(&mut self, c: char, pos: Position) {
        self.marks.insert(c, pos);
    }

    /// A mark's position. `'` and `` ` `` mean "where the last jump came from".
    pub fn mark(&self, c: char) -> Option<Position> {
        match c {
            '\'' | '`' => self.jumps.get(self.index.checked_sub(1)?).copied(),
            c => self.marks.get(&c).copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(page: usize) -> Position {
        Position {
            page,
            offset: 0.0,
            x: 0.0,
        }
    }

    #[test]
    fn back_and_forward() {
        let mut j = JumpList::default();
        j.push(p(1)); // jumped from 1 to 10
        j.push(p(10)); // jumped from 10 to 50
        assert_eq!(j.back(p(50), 1), Some(p(10)));
        assert_eq!(j.back(p(10), 1), Some(p(1)));
        assert_eq!(j.back(p(1), 1), None);
        assert_eq!(j.forward(1), Some(p(10)));
        assert_eq!(j.forward(1), Some(p(50)));
        assert_eq!(j.forward(1), None);
    }

    #[test]
    fn new_jump_forgets_forward_history() {
        let mut j = JumpList::default();
        j.push(p(1));
        j.push(p(2));
        assert_eq!(j.back(p(3), 2), Some(p(1)));
        j.push(p(1)); // jump from 1 somewhere else
        assert_eq!(j.forward(1), None);
        assert_eq!(j.back(p(7), 1), Some(p(1)));
    }

    #[test]
    fn marks_and_last_jump() {
        let mut j = JumpList::default();
        assert_eq!(j.mark('a'), None);
        j.set_mark('a', p(4));
        assert_eq!(j.mark('a'), Some(p(4)));
        assert_eq!(j.mark('\''), None);
        j.push(p(9));
        assert_eq!(j.mark('\''), Some(p(9)));
    }
}
