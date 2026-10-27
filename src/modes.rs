/// Input mode: decides where key presses go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// Typing in the `:` command line.
    Command,
    /// Typing a `/` search.
    Search,
    /// Link hint labels on screen (`f`).
    Hints,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Command => "COMMAND",
            Mode::Search => "SEARCH",
            Mode::Hints => "HINTS",
        }
    }
}
