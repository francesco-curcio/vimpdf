use crate::links::LinkTarget;

/// Every user action, whichever input produced it.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Scroll by `n` steps.
    ScrollDown(u32),
    ScrollUp(u32),
    ScrollLeft(u32),
    ScrollRight(u32),
    /// Scroll by a distance in screen points (trackpad / mouse wheel).
    ScrollBy {
        dx: f32,
        dy: f32,
    },
    HalfPageDown(u32),
    HalfPageUp(u32),
    ScreenDown(u32),
    ScreenUp(u32),
    NextPage(u32),
    PrevPage(u32),
    GotoFirst,
    GotoLast,
    /// 1-based page number.
    GotoPage(usize),
    /// Zoom in / out by `n` steps, anchored at the view centre.
    ZoomIn(u32),
    ZoomOut(u32),
    /// Zoom to 100%.
    ZoomReset,
    /// Multiply the zoom, anchored at a point in view coordinates
    /// (pinch / Cmd+wheel).
    ZoomBy {
        factor: f32,
        x: f32,
        y: f32,
    },
    FitWidth,
    FitPage,
    FollowLink(LinkTarget),
    Cancel,
    Quit,
}

impl Command {
    /// Apply a vim count prefix (`5j`, `12G`).
    pub fn with_count(self, n: u32) -> Self {
        use Command::*;
        match self {
            ScrollDown(k) => ScrollDown(k * n),
            ScrollUp(k) => ScrollUp(k * n),
            ScrollLeft(k) => ScrollLeft(k * n),
            ScrollRight(k) => ScrollRight(k * n),
            HalfPageDown(k) => HalfPageDown(k * n),
            HalfPageUp(k) => HalfPageUp(k * n),
            ScreenDown(k) => ScreenDown(k * n),
            ScreenUp(k) => ScreenUp(k * n),
            NextPage(k) => NextPage(k * n),
            ZoomIn(k) => ZoomIn(k * n),
            ZoomOut(k) => ZoomOut(k * n),
            PrevPage(k) => PrevPage(k * n),
            GotoFirst | GotoLast | GotoPage(_) => GotoPage(n as usize),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_multiply_motions_and_select_pages() {
        assert_eq!(Command::ScrollDown(1).with_count(5), Command::ScrollDown(5));
        assert_eq!(Command::NextPage(2).with_count(3), Command::NextPage(6));
        assert_eq!(Command::GotoLast.with_count(12), Command::GotoPage(12));
        assert_eq!(Command::GotoFirst.with_count(3), Command::GotoPage(3));
        assert_eq!(Command::Quit.with_count(3), Command::Quit);
    }
}
