/// A vim count prefix being typed ("12" in "12G").
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Count(Option<u32>);

/// Large enough for any page number, small enough to never overflow.
const MAX_COUNT: u32 = 99_999;

impl Count {
    /// Feed a key; returns true if it was consumed as a digit. A leading `0`
    /// is not a count (it stays available as a binding, like in vim).
    pub fn feed(&mut self, key: &str) -> bool {
        let mut chars = key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return false;
        };
        let Some(d) = c.to_digit(10) else {
            return false;
        };
        if d == 0 && self.0.is_none() {
            return false;
        }
        self.0 = Some((self.0.unwrap_or(0) * 10 + d).min(MAX_COUNT));
        true
    }

    pub fn get(&self) -> Option<u32> {
        self.0
    }

    pub fn take(&mut self) -> Option<u32> {
        self.0.take()
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_digits() {
        let mut c = Count::default();
        assert!(c.feed("1"));
        assert!(c.feed("0"));
        assert!(c.feed("5"));
        assert_eq!(c.take(), Some(105));
        assert_eq!(c.get(), None);
    }

    #[test]
    fn leading_zero_and_non_digits_are_not_counts() {
        let mut c = Count::default();
        assert!(!c.feed("0"));
        assert!(!c.feed("j"));
        assert!(!c.feed("<C-d>"));
        assert_eq!(c.get(), None);
    }

    #[test]
    fn saturates() {
        let mut c = Count::default();
        for _ in 0..12 {
            c.feed("9");
        }
        assert_eq!(c.get(), Some(MAX_COUNT));
    }
}
