use std::path::PathBuf;

use crate::links::{HintAction, LinkTarget};

/// Every user action, whichever input produced it: key bindings, the `:`
/// command line, the mouse, toolbar buttons.
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
    /// Show labels on visible links; typing one follows (or copies) it.
    Hints(HintAction),
    /// `<C-o>` / `<C-i>`: move through the jump history.
    JumpBack(u32),
    JumpForward(u32),
    /// Wait for a letter, then set (`set: true`) or go to that mark.
    AwaitMark {
        set: bool,
    },
    SetMark(char),
    GotoMark(char),
    /// Open the `:` command line, optionally prefilled.
    EnterCommandLine(String),
    /// Open the `/` search line, optionally prefilled.
    EnterSearch(String),
    /// Search for text; an empty query repeats the last search.
    Search(String),
    /// Go to the `n`th next / previous search match.
    SearchNext(u32),
    SearchPrev(u32),
    ToggleStatusbar,
    ToggleToolbar,
    ToggleHelp,
    ToggleFullscreen,
    /// Re-open the current file, keeping the position.
    Reload,
    Open(PathBuf),
    /// Set a config option (`:set zoom.step 1.2`).
    Set {
        option: String,
        value: String,
    },
    /// Reload the config file.
    Source,
    Cancel,
    Quit,
}

/// Commands that take an optional repeat count, by config name.
type MakeCounted = fn(u32) -> Command;

const COUNTED: &[(&str, MakeCounted)] = &[
    ("scroll_down", Command::ScrollDown),
    ("scroll_up", Command::ScrollUp),
    ("scroll_left", Command::ScrollLeft),
    ("scroll_right", Command::ScrollRight),
    ("half_page_down", Command::HalfPageDown),
    ("half_page_up", Command::HalfPageUp),
    ("screen_down", Command::ScreenDown),
    ("screen_up", Command::ScreenUp),
    ("next_page", Command::NextPage),
    ("prev_page", Command::PrevPage),
    ("zoom_in", Command::ZoomIn),
    ("zoom_out", Command::ZoomOut),
    ("jump_back", Command::JumpBack),
    ("jump_forward", Command::JumpForward),
    ("search_next", Command::SearchNext),
    ("search_prev", Command::SearchPrev),
];

/// Commands without arguments, by config name.
const SIMPLE: &[(&str, Command)] = &[
    ("goto_first", Command::GotoFirst),
    ("goto_last", Command::GotoLast),
    ("zoom_reset", Command::ZoomReset),
    ("fit_width", Command::FitWidth),
    ("fit_page", Command::FitPage),
    ("hints", Command::Hints(HintAction::Follow)),
    ("hints_copy", Command::Hints(HintAction::CopyAddress)),
    ("set_mark", Command::AwaitMark { set: true }),
    ("goto_mark", Command::AwaitMark { set: false }),
    ("toggle_statusbar", Command::ToggleStatusbar),
    ("toggle_toolbar", Command::ToggleToolbar),
    ("toggle_help", Command::ToggleHelp),
    ("toggle_fullscreen", Command::ToggleFullscreen),
    ("reload", Command::Reload),
    ("source", Command::Source),
    ("cancel", Command::Cancel),
    ("quit", Command::Quit),
];

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
            PrevPage(k) => PrevPage(k * n),
            ZoomIn(k) => ZoomIn(k * n),
            ZoomOut(k) => ZoomOut(k * n),
            SearchNext(k) => SearchNext(k * n),
            JumpBack(k) => JumpBack(k * n),
            JumpForward(k) => JumpForward(k * n),
            SearchPrev(k) => SearchPrev(k * n),
            GotoFirst | GotoLast | GotoPage(_) => GotoPage(n as usize),
            other => other,
        }
    }

    /// Parse a binding action like "scroll_down 3" or "fit_width".
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let (name, arg) = match s.split_once(char::is_whitespace) {
            Some((n, a)) => (n, Some(a.trim())),
            None => (s, None),
        };
        if let Some((_, make)) = COUNTED.iter().find(|(n, _)| *n == name) {
            let n = match arg {
                None => 1,
                Some(a) => a
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| format!("'{name}' takes a positive count, got '{a}'"))?,
            };
            return Ok(make(n));
        }
        let mark_letter = |a: &str| {
            let mut chars = a.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(c),
                _ => Err(format!("'{name}' takes one letter, got '{a}'")),
            }
        };
        match (name, arg) {
            ("set_mark", Some(a)) => return mark_letter(a).map(Command::SetMark),
            ("goto_mark", Some(a)) => return mark_letter(a).map(Command::GotoMark),
            _ => {}
        }
        if let Some((_, cmd)) = SIMPLE.iter().find(|(n, _)| *n == name) {
            return match arg {
                None => Ok(cmd.clone()),
                Some(a) => Err(format!("'{name}' takes no argument, got '{a}'")),
            };
        }
        match (name, arg) {
            ("goto_page", Some(a)) => a
                .parse::<usize>()
                .map(Command::GotoPage)
                .map_err(|_| format!("'goto_page' takes a page number, got '{a}'")),
            ("command_line", a) => Ok(Command::EnterCommandLine(a.unwrap_or("").to_owned())),
            ("search_prompt", a) => Ok(Command::EnterSearch(a.unwrap_or("").to_owned())),
            ("search", a) => Ok(Command::Search(a.unwrap_or("").to_owned())),
            ("open", Some(a)) => Ok(Command::Open(expand_tilde(a))),
            ("", _) => Err("empty command".into()),
            _ => Err(format!("unknown command '{s}'")),
        }
    }

    /// The config name of a command, for the help overlay.
    pub fn describe(&self) -> String {
        if let Some((name, _)) = SIMPLE.iter().find(|(_, c)| c == self) {
            return (*name).to_owned();
        }
        if let Some(n) = self.count() {
            for (name, make) in COUNTED {
                if make(n) == *self {
                    return if n == 1 {
                        (*name).to_owned()
                    } else {
                        format!("{name} {n}")
                    };
                }
            }
        }
        match self {
            Command::GotoPage(n) => format!("goto_page {n}"),
            Command::EnterCommandLine(s) if s.is_empty() => "command_line".into(),
            Command::EnterCommandLine(s) => format!("command_line {s}"),
            Command::EnterSearch(s) if s.is_empty() => "search_prompt".into(),
            Command::EnterSearch(s) => format!("search_prompt {s}"),
            Command::Search(s) => format!("search {s}"),
            Command::SetMark(c) => format!("set_mark {c}"),
            Command::GotoMark(c) => format!("goto_mark {c}"),
            Command::Open(p) => format!("open {}", p.display()),
            other => format!("{other:?}"),
        }
    }

    fn count(&self) -> Option<u32> {
        use Command::*;
        match self {
            ScrollDown(n) | ScrollUp(n) | ScrollLeft(n) | ScrollRight(n) | HalfPageDown(n)
            | HalfPageUp(n) | ScreenDown(n) | ScreenUp(n) | NextPage(n) | PrevPage(n)
            | ZoomIn(n) | ZoomOut(n) | SearchNext(n) | SearchPrev(n) | JumpBack(n)
            | JumpForward(n) => Some(*n),
            _ => None,
        }
    }
}

/// Parse a `:` command line. `Ok(None)` for an empty line.
pub fn parse_ex(line: &str) -> Result<Option<Command>, String> {
    let line = line.trim().trim_start_matches(':').trim();
    if line.is_empty() {
        return Ok(None);
    }
    if let Ok(n) = line.parse::<usize>() {
        return Ok(Some(Command::GotoPage(n)));
    }
    let (word, rest) = match line.split_once(char::is_whitespace) {
        Some((w, r)) => (w, r.trim()),
        None => (line, ""),
    };
    let cmd = match word {
        "q" | "q!" | "quit" | "qa" | "x" => Command::Quit,
        "o" | "open" | "e" | "edit" if rest.is_empty() => {
            return Err(format!(":{word} needs a file path"));
        }
        "o" | "open" | "e" | "edit" => Command::Open(expand_tilde(rest)),
        "so" | "source" => Command::Source,
        "h" | "help" => Command::ToggleHelp,
        "set" | "se" => return parse_set(rest).map(Some),
        _ => Command::parse(line)?,
    };
    Ok(Some(cmd))
}

/// `set opt value`, `set opt=value`, `set opt` (true), `set noopt` (false).
fn parse_set(args: &str) -> Result<Command, String> {
    if args.is_empty() {
        return Err(":set needs an option".into());
    }
    let (option, value) = match args.split_once(['=', ' ', '\t']) {
        Some((o, v)) => (o.trim(), v.trim().to_owned()),
        None => match args.strip_prefix("no") {
            Some(o) => (o, "false".to_owned()),
            None => (args, "true".to_owned()),
        },
    };
    Ok(Command::Set {
        option: option.to_owned(),
        value,
    })
}

pub fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => directories::BaseDirs::new()
            .map(|d| d.home_dir().join(rest))
            .unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
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

    #[test]
    fn parses_binding_actions() {
        assert_eq!(Command::parse("scroll_down"), Ok(Command::ScrollDown(1)));
        assert_eq!(Command::parse("scroll_down 3"), Ok(Command::ScrollDown(3)));
        assert_eq!(Command::parse("fit_width"), Ok(Command::FitWidth));
        assert_eq!(Command::parse("goto_page 7"), Ok(Command::GotoPage(7)));
        assert_eq!(
            Command::parse("toggle_fullscreen"),
            Ok(Command::ToggleFullscreen)
        );
        assert!(Command::parse("scroll_down 0").is_err());
        assert!(Command::parse("scroll_down x").is_err());
        assert!(Command::parse("fit_width 2").is_err());
        assert!(Command::parse("teleport").is_err());
        assert!(Command::parse("").is_err());
    }

    #[test]
    fn describe_round_trips_through_parse() {
        for s in [
            "scroll_down",
            "scroll_down 3",
            "zoom_in",
            "goto_first",
            "goto_page 4",
            "toggle_help",
            "search_prompt",
            "search_next 2",
            "hints",
            "hints_copy",
            "jump_back",
            "set_mark",
            "set_mark a",
            "command_line",
            "quit",
        ] {
            let cmd = Command::parse(s).unwrap();
            assert_eq!(cmd.describe(), s);
        }
    }

    #[test]
    fn parses_ex_commands() {
        assert_eq!(parse_ex(""), Ok(None));
        assert_eq!(parse_ex(":12"), Ok(Some(Command::GotoPage(12))));
        assert_eq!(parse_ex("q"), Ok(Some(Command::Quit)));
        assert_eq!(parse_ex("source"), Ok(Some(Command::Source)));
        assert_eq!(
            parse_ex("open /tmp/a.pdf"),
            Ok(Some(Command::Open("/tmp/a.pdf".into())))
        );
        assert!(parse_ex("open").is_err());
        assert_eq!(parse_ex("zoom_in 2"), Ok(Some(Command::ZoomIn(2))));
        assert_eq!(
            parse_ex("search Standard Model"),
            Ok(Some(Command::Search("Standard Model".into())))
        );
        assert!(parse_ex("frobnicate").is_err());
    }

    #[test]
    fn parses_set_forms() {
        let set = |o: &str, v: &str| {
            Ok(Some(Command::Set {
                option: o.into(),
                value: v.into(),
            }))
        };
        assert_eq!(parse_ex("set zoom.step 1.2"), set("zoom.step", "1.2"));
        assert_eq!(parse_ex("set zoom.step=1.2"), set("zoom.step", "1.2"));
        assert_eq!(parse_ex("set toolbar"), set("toolbar", "true"));
        assert_eq!(parse_ex("set notoolbar"), set("toolbar", "false"));
        assert!(parse_ex("set").is_err());
    }
}
