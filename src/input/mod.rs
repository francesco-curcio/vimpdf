//! Turning egui input into `Command`s. Everything below this module except
//! the egui event translation is pure and unit-tested.

pub mod command;
pub mod counts;
pub mod keymap;
pub mod mouse;

use eframe::egui::{Event, Key, Modifiers};

/// Translate egui events into vim-style key tokens, in order. Printable
/// characters come from `Text` events (so "G", "+", "'" follow the keyboard
/// layout); keys with Ctrl/Cmd/Alt and special keys come from `Key` events.
pub fn key_tokens(events: &[Event]) -> Vec<String> {
    let mut out = Vec::new();
    for event in events {
        match event {
            Event::Text(text) => {
                out.extend(
                    text.chars()
                        .filter(|c| *c != ' ' && !c.is_control())
                        .map(|c| c.to_string()),
                );
            }
            // Cmd+C (Ctrl+C elsewhere) arrives as a copy event, not a key.
            Event::Copy => out.push("<Copy>".to_owned()),
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if let Some(tok) = key_token(*key, *modifiers) {
                    out.push(tok);
                }
            }
            _ => {}
        }
    }
    out
}

fn key_token(key: Key, m: Modifiers) -> Option<String> {
    let special = match key {
        Key::Space => Some("Space"),
        Key::Escape => Some("Esc"),
        Key::Tab => Some("Tab"),
        Key::Enter => Some("CR"),
        Key::Backspace => Some("BS"),
        Key::Delete => Some("Del"),
        Key::ArrowUp => Some("Up"),
        Key::ArrowDown => Some("Down"),
        Key::ArrowLeft => Some("Left"),
        Key::ArrowRight => Some("Right"),
        Key::Home => Some("Home"),
        Key::End => Some("End"),
        Key::PageUp => Some("PageUp"),
        Key::PageDown => Some("PageDown"),
        Key::F1 => Some("F1"),
        Key::F2 => Some("F2"),
        Key::F3 => Some("F3"),
        Key::F4 => Some("F4"),
        Key::F5 => Some("F5"),
        Key::F6 => Some("F6"),
        Key::F7 => Some("F7"),
        Key::F8 => Some("F8"),
        Key::F9 => Some("F9"),
        Key::F10 => Some("F10"),
        Key::F11 => Some("F11"),
        Key::F12 => Some("F12"),
        _ => None,
    };
    let chorded = m.ctrl || m.alt || m.mac_cmd;

    let name = match special {
        Some(name) => name.to_owned(),
        // Plain printable keys arrive as Text events instead.
        None if !chorded => return None,
        None => {
            let name = key.name();
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphabetic() => {
                    // Shift is folded into the letter's case, like vim.
                    let c = if m.shift {
                        c.to_ascii_uppercase()
                    } else {
                        c.to_ascii_lowercase()
                    };
                    return Some(wrap(m, &c.to_string(), false));
                }
                _ => name.to_owned(),
            }
        }
    };
    Some(wrap(m, &name, m.shift))
}

fn wrap(m: Modifiers, key: &str, shift: bool) -> String {
    let mut s = String::from("<");
    for (on, p) in [
        (m.ctrl, "C-"),
        (m.alt, "A-"),
        (m.mac_cmd, "D-"),
        (shift, "S-"),
    ] {
        if on {
            s.push_str(p);
        }
    }
    if s.len() == 1 && key.chars().count() == 1 {
        return key.to_owned();
    }
    s.push_str(key);
    s.push('>');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn tokens_match_keymap_notation() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        let events = vec![
            key(Key::J, Modifiers::NONE),
            Event::Text("j".into()),
            key(Key::D, ctrl),
            key(Key::F, ctrl_shift),
            key(Key::Space, Modifiers::NONE),
            Event::Text(" ".into()),
            key(Key::Space, Modifiers::SHIFT),
            Event::Text("G".into()),
            key(Key::Escape, Modifiers::NONE),
            key(Key::F10, Modifiers::NONE),
        ];
        let toks = key_tokens(&events);
        assert_eq!(
            toks,
            vec![
                "j",
                "<C-d>",
                "<C-F>",
                "<Space>",
                "<S-Space>",
                "G",
                "<Esc>",
                "<F10>"
            ]
        );
        for t in &toks {
            assert_eq!(&keymap::parse_sequence(t).unwrap()[0], t);
        }
    }
}
