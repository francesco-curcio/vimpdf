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
    /// Selecting text (`v` / `V`, or a mouse selection).
    Visual,
    /// The table of contents panel has the keyboard.
    Outline,
    /// The annotations panel has the keyboard.
    Annotations,
    /// Writing a comment.
    Comment,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Command => "COMMAND",
            Mode::Search => "SEARCH",
            Mode::Hints => "HINTS",
            Mode::Visual => "VISUAL",
            Mode::Outline => "OUTLINE",
            Mode::Annotations => "ANNOTATIONS",
            Mode::Comment => "COMMENT",
        }
    }
}
