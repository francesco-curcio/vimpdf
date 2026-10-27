//! Key sequences and bindings. Keys are normalised to vim-style tokens:
//! printable characters as themselves ("j", "G", "+"), everything else in
//! angle brackets ("<C-d>", "<C-F>", "<Space>", "<S-Space>", "<Esc>").

use std::collections::{HashMap, HashSet};

use super::command::Command;
use super::counts::Count;

pub type KeySeq = Vec<String>;

/// Parse a binding like "gg", "<C-d>", "m<Tab>" into normalised tokens.
pub fn parse_sequence(s: &str) -> Result<KeySeq, String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '<'
            && let Some(end) = rest.find('>')
            && end > 1
        {
            out.push(normalize_special(&rest[1..end])?);
            rest = &rest[end + 1..];
            continue;
        }
        out.push(c.to_string());
        rest = &rest[c.len_utf8()..];
    }
    if out.is_empty() {
        return Err("empty key sequence".into());
    }
    Ok(out)
}

/// Normalise the inside of "<...>": modifier order C, A, D, S; shift on a
/// letter becomes an uppercase letter ("<C-S-f>" == "<C-F>").
fn normalize_special(inner: &str) -> Result<String, String> {
    let mut parts: Vec<&str> = inner.split('-').collect();
    // "<C-->" means Ctrl + '-'.
    if inner.ends_with("--") {
        parts.truncate(parts.len() - 2);
        parts.push("-");
    }
    let key = parts.pop().filter(|k| !k.is_empty()).ok_or("missing key")?;
    let (mut ctrl, mut alt, mut cmd, mut shift) = (false, false, false, false);
    for m in parts {
        match m.to_ascii_uppercase().as_str() {
            "C" => ctrl = true,
            "A" | "M" => alt = true,
            "D" => cmd = true,
            "S" => shift = true,
            other => return Err(format!("unknown modifier '{other}' in <{inner}>")),
        }
    }

    let mut key = if key.chars().count() == 1 {
        key.to_owned()
    } else {
        canonical_key_name(key).ok_or_else(|| format!("unknown key <{inner}>"))?
    };
    if key.chars().count() == 1 {
        let c = key.chars().next().unwrap();
        if shift && c.is_ascii_alphabetic() {
            key = c.to_ascii_uppercase().to_string();
            shift = false;
        }
        if !(ctrl || alt || cmd || shift) {
            return Ok(key);
        }
    }

    let mut s = String::from("<");
    for (on, m) in [(ctrl, "C-"), (alt, "A-"), (cmd, "D-"), (shift, "S-")] {
        if on {
            s.push_str(m);
        }
    }
    s.push_str(&key);
    s.push('>');
    Ok(s)
}

fn canonical_key_name(name: &str) -> Option<String> {
    const NAMES: &[&str] = &[
        "Space", "Esc", "Tab", "CR", "BS", "Del", "Up", "Down", "Left", "Right", "Home", "End",
        "PageUp", "PageDown", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11",
        "F12",
    ];
    let lower = name.to_ascii_lowercase();
    let alias = match lower.as_str() {
        "enter" | "return" => "CR",
        "escape" => "Esc",
        "backspace" => "BS",
        "delete" => "Del",
        "lt" => return Some("<".into()),
        _ => "",
    };
    if !alias.is_empty() {
        return Some(alias.into());
    }
    NAMES
        .iter()
        .find(|n| n.to_ascii_lowercase() == lower)
        .map(|n| (*n).to_owned())
}

#[derive(Debug, Default, Clone)]
pub struct Keymap {
    bindings: HashMap<KeySeq, Command>,
    prefixes: HashSet<KeySeq>,
}

enum Lookup<'a> {
    Exact(&'a Command),
    Prefix,
    None,
}

impl Keymap {
    pub fn bind(&mut self, seq: &str, cmd: Command) {
        let seq = parse_sequence(seq).expect("invalid built-in key sequence");
        for i in 1..seq.len() {
            self.prefixes.insert(seq[..i].to_vec());
        }
        self.bindings.insert(seq, cmd);
    }

    fn lookup(&self, seq: &[String]) -> Lookup<'_> {
        // A sequence that is both bound and a prefix waits for more keys.
        if self.prefixes.contains(seq) {
            Lookup::Prefix
        } else if let Some(cmd) = self.bindings.get(seq) {
            Lookup::Exact(cmd)
        } else {
            Lookup::None
        }
    }

    pub fn default_normal() -> Self {
        use Command::*;
        let mut k = Self::default();
        for (seq, cmd) in [
            ("j", ScrollDown(1)),
            ("<Down>", ScrollDown(1)),
            ("k", ScrollUp(1)),
            ("<Up>", ScrollUp(1)),
            ("h", ScrollLeft(1)),
            ("<Left>", ScrollLeft(1)),
            ("l", ScrollRight(1)),
            ("<Right>", ScrollRight(1)),
            ("<C-d>", HalfPageDown(1)),
            ("<C-u>", HalfPageUp(1)),
            ("<C-f>", ScreenDown(1)),
            ("<C-b>", ScreenUp(1)),
            ("<Space>", ScreenDown(1)),
            ("<S-Space>", ScreenUp(1)),
            ("<PageDown>", ScreenDown(1)),
            ("<PageUp>", ScreenUp(1)),
            ("J", NextPage(1)),
            ("K", PrevPage(1)),
            ("gg", GotoFirst),
            ("<Home>", GotoFirst),
            ("G", GotoLast),
            ("<End>", GotoLast),
            ("+", ZoomIn(1)),
            ("-", ZoomOut(1)),
            ("=", ZoomReset),
            ("s", FitWidth),
            ("a", FitPage),
            ("<Esc>", Cancel),
            ("q", Quit),
        ] {
            k.bind(seq, cmd);
        }
        k
    }
}

/// Pending count and partial key sequence.
#[derive(Debug, Default)]
pub struct KeyState {
    count: Count,
    pending: KeySeq,
}

impl KeyState {
    /// Feed one key token. Returns a command once a binding is complete.
    pub fn feed(&mut self, key: &str, keymap: &Keymap) -> Option<Command> {
        if self.pending.is_empty() && self.count.feed(key) {
            return None;
        }
        if key == "<Esc>" && self.is_pending() {
            self.reset();
            return None;
        }

        self.pending.push(key.to_owned());
        match keymap.lookup(&self.pending) {
            Lookup::Prefix => None,
            Lookup::Exact(cmd) => {
                let cmd = cmd.clone();
                let count = self.count.take();
                self.pending.clear();
                Some(match count {
                    Some(n) => cmd.with_count(n),
                    None => cmd,
                })
            }
            Lookup::None => {
                self.reset();
                None
            }
        }
    }

    pub fn is_pending(&self) -> bool {
        self.count.get().is_some() || !self.pending.is_empty()
    }

    pub fn reset(&mut self) {
        self.count.clear();
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(state: &mut KeyState, keymap: &Keymap, keys: &[&str]) -> Vec<Command> {
        keys.iter().filter_map(|k| state.feed(k, keymap)).collect()
    }

    #[test]
    fn parses_sequences() {
        assert_eq!(parse_sequence("gg").unwrap(), vec!["g", "g"]);
        assert_eq!(parse_sequence("<C-d>").unwrap(), vec!["<C-d>"]);
        assert_eq!(parse_sequence("<c-S-f>").unwrap(), vec!["<C-F>"]);
        assert_eq!(parse_sequence("<s-space>").unwrap(), vec!["<S-Space>"]);
        assert_eq!(parse_sequence("<space>").unwrap(), vec!["<Space>"]);
        assert_eq!(parse_sequence("m<Tab>").unwrap(), vec!["m", "<Tab>"]);
        assert_eq!(parse_sequence("<C-->").unwrap(), vec!["<C-->"]);
        assert_eq!(parse_sequence("<").unwrap(), vec!["<"]);
        assert_eq!(parse_sequence("<lt>").unwrap(), vec!["<"]);
        assert!(parse_sequence("<X-a>").is_err());
        assert!(parse_sequence("<Bogus>").is_err());
        assert!(parse_sequence("").is_err());
    }

    #[test]
    fn single_keys_and_sequences() {
        let km = Keymap::default_normal();
        let mut st = KeyState::default();
        assert_eq!(feed_all(&mut st, &km, &["j"]), vec![Command::ScrollDown(1)]);
        assert_eq!(feed_all(&mut st, &km, &["g"]), vec![]);
        assert!(st.is_pending());
        assert_eq!(feed_all(&mut st, &km, &["g"]), vec![Command::GotoFirst]);
        assert!(!st.is_pending());
    }

    #[test]
    fn counts_apply_to_commands() {
        let km = Keymap::default_normal();
        let mut st = KeyState::default();
        assert_eq!(
            feed_all(&mut st, &km, &["5", "j"]),
            vec![Command::ScrollDown(5)]
        );
        assert_eq!(
            feed_all(&mut st, &km, &["1", "2", "G"]),
            vec![Command::GotoPage(12)]
        );
        assert_eq!(
            feed_all(&mut st, &km, &["3", "g", "g"]),
            vec![Command::GotoPage(3)]
        );
    }

    #[test]
    fn unknown_sequence_and_escape_reset() {
        let km = Keymap::default_normal();
        let mut st = KeyState::default();
        assert_eq!(feed_all(&mut st, &km, &["g", "x"]), vec![]);
        assert!(!st.is_pending());
        assert_eq!(feed_all(&mut st, &km, &["j"]), vec![Command::ScrollDown(1)]);

        assert_eq!(feed_all(&mut st, &km, &["4", "<Esc>"]), vec![]);
        assert!(!st.is_pending());
        assert_eq!(feed_all(&mut st, &km, &["<Esc>"]), vec![Command::Cancel]);
    }
}
