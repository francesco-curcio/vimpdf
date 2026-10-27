//! Configuration: defaults, loading `~/.config/pdfv/config.toml`, and `:set`.
//!
//! Every option goes through [`Config::set`], so the config file and `:set`
//! accept exactly the same values. Invalid entries never abort loading: they
//! produce a warning and the default is kept.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::input::command::Command;
use crate::input::keymap::{Keymap, parse_sequence};
use crate::viewport::FitMode;

/// An RGBA colour, from "#rrggbb" or "#rrggbbaa".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub [u8; 4]);

impl Color {
    pub fn parse(s: &str) -> Result<Self, String> {
        let hex = s
            .strip_prefix('#')
            .filter(|h| (h.len() == 6 || h.len() == 8) && h.is_ascii())
            .ok_or_else(|| format!("expected a colour like \"#rrggbb\", got \"{s}\""))?;
        let byte = |i: usize| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| format!("invalid hex colour \"{s}\""))
        };
        let a = if hex.len() == 8 { byte(6)? } else { 255 };
        Ok(Color([byte(0)?, byte(2)?, byte(4)?, a]))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragMode {
    Select,
    Pan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Cmd,
    Ctrl,
    Alt,
    Shift,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    pub page_background: Color,
    pub recolor_dark: Color,
    pub recolor_light: Color,
    pub search_highlight: Color,
    pub selection: Color,
    pub caret: Color,
    pub hint_background: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub font_family: String,
    pub font_size: f32,
    pub statusbar: bool,
    pub toolbar: bool,
    pub start_maximised: bool,
    pub colors: Colors,
    pub scroll_step: f32,
    pub smooth_scroll: bool,
    pub zoom_step: f32,
    pub default_fit: FitMode,
    pub drag: DragMode,
    pub pan_modifier: Modifier,
    pub zoom_modifier: Modifier,
    pub copy_on_select: bool,
    /// Key binding overrides for normal mode, in file order.
    pub normal_keys: Vec<(String, String)>,
}

impl Default for Config {
    fn default() -> Self {
        let c = |s| Color::parse(s).expect("valid default colour");
        Self {
            font_family: "Menlo".into(),
            font_size: 14.0,
            statusbar: true,
            toolbar: false,
            start_maximised: true,
            colors: Colors {
                page_background: c("#ffffff"),
                recolor_dark: c("#1e1e1e"),
                recolor_light: c("#d4d4d4"),
                search_highlight: c("#f9e2af80"),
                selection: c("#89b4fa60"),
                caret: c("#f38ba8"),
                hint_background: c("#fab387"),
            },
            scroll_step: 60.0,
            smooth_scroll: true,
            zoom_step: 1.1,
            default_fit: FitMode::Width,
            drag: DragMode::Select,
            pan_modifier: Modifier::Alt,
            zoom_modifier: Modifier::Cmd,
            copy_on_select: false,
            normal_keys: Vec::new(),
        }
    }
}

/// Every option, as "section.name".
pub const OPTIONS: &[&str] = &[
    "ui.font_family",
    "ui.font_size",
    "ui.statusbar",
    "ui.toolbar",
    "ui.start_maximised",
    "colors.page_background",
    "colors.recolor_dark",
    "colors.recolor_light",
    "colors.search_highlight",
    "colors.selection",
    "colors.caret",
    "colors.hint_background",
    "scroll.step",
    "scroll.smooth",
    "zoom.step",
    "zoom.default_fit",
    "mouse.drag",
    "mouse.pan_modifier",
    "mouse.zoom_modifier",
    "mouse.copy_on_select",
];

fn as_bool(v: &Value) -> Result<bool, String> {
    v.as_bool()
        .ok_or_else(|| format!("expected true or false, got {v}"))
}

fn as_str(v: &Value) -> Result<&str, String> {
    v.as_str()
        .ok_or_else(|| format!("expected a string, got {v}"))
}

fn as_num(v: &Value, min: f64, max: f64) -> Result<f32, String> {
    let n = v
        .as_float()
        .or_else(|| v.as_integer().map(|i| i as f64))
        .ok_or_else(|| format!("expected a number, got {v}"))?;
    if !(min..=max).contains(&n) {
        return Err(format!(
            "expected a number between {min} and {max}, got {n}"
        ));
    }
    Ok(n as f32)
}

fn as_color(v: &Value) -> Result<Color, String> {
    Color::parse(as_str(v)?)
}

fn as_modifier(v: &Value) -> Result<Modifier, String> {
    match as_str(v)? {
        "cmd" => Ok(Modifier::Cmd),
        "ctrl" => Ok(Modifier::Ctrl),
        "alt" => Ok(Modifier::Alt),
        "shift" => Ok(Modifier::Shift),
        other => Err(format!(
            "expected \"cmd\", \"ctrl\", \"alt\" or \"shift\", got \"{other}\""
        )),
    }
}

impl Config {
    /// `$XDG_CONFIG_HOME/pdfv/config.toml`, else `~/.config/pdfv/config.toml`
    /// (also on macOS, to live next to other dotfiles).
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".config")))?;
        Some(base.join("pdfv").join("config.toml"))
    }

    /// Load a config file. A missing file gives the defaults silently.
    pub fn load(path: &Path) -> (Self, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Self::default(), Vec::new()),
            Err(e) => (
                Self::default(),
                vec![format!("cannot read {}: {e}", path.display())],
            ),
        }
    }

    pub fn from_toml(text: &str) -> (Self, Vec<String>) {
        let mut config = Self::default();
        let mut warnings = Vec::new();
        let table: Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                let msg = e.to_string();
                let first = msg.lines().next().unwrap_or("syntax error");
                return (config, vec![format!("config: {first}; using defaults")]);
            }
        };

        for (section, value) in &table {
            let Some(entries) = value.as_table() else {
                warnings.push(format!("config: '{section}' should be a [section]"));
                continue;
            };
            if section == "keys" {
                config.load_keys(entries, &mut warnings);
                continue;
            }
            for (name, value) in entries {
                let key = format!("{section}.{name}");
                if let Err(e) = config.set(&key, value) {
                    warnings.push(format!("config: {key}: {e}"));
                }
            }
        }
        (config, warnings)
    }

    fn load_keys(&mut self, modes: &Table, warnings: &mut Vec<String>) {
        for (mode, bindings) in modes {
            let Some(bindings) = bindings.as_table() else {
                warnings.push(format!("config: keys.{mode} should be a table"));
                continue;
            };
            if mode != "normal" {
                warnings.push(format!(
                    "config: keys.{mode}: '{mode}' mode doesn't exist yet; ignored"
                ));
                continue;
            }
            for (seq, action) in bindings {
                match action.as_str() {
                    Some(a) => self.normal_keys.push((seq.clone(), a.to_owned())),
                    None => warnings.push(format!(
                        "config: keys.{mode}.\"{seq}\": expected a command string"
                    )),
                }
            }
        }
    }

    /// Set one option by its full "section.name" key.
    pub fn set(&mut self, key: &str, v: &Value) -> Result<(), String> {
        match key {
            "ui.font_family" => self.font_family = as_str(v)?.to_owned(),
            "ui.font_size" => self.font_size = as_num(v, 6.0, 72.0)?,
            "ui.statusbar" => self.statusbar = as_bool(v)?,
            "ui.toolbar" => self.toolbar = as_bool(v)?,
            "ui.start_maximised" => self.start_maximised = as_bool(v)?,
            "colors.page_background" => self.colors.page_background = as_color(v)?,
            "colors.recolor_dark" => self.colors.recolor_dark = as_color(v)?,
            "colors.recolor_light" => self.colors.recolor_light = as_color(v)?,
            "colors.search_highlight" => self.colors.search_highlight = as_color(v)?,
            "colors.selection" => self.colors.selection = as_color(v)?,
            "colors.caret" => self.colors.caret = as_color(v)?,
            "colors.hint_background" => self.colors.hint_background = as_color(v)?,
            "scroll.step" => self.scroll_step = as_num(v, 1.0, 10_000.0)?,
            "scroll.smooth" => self.smooth_scroll = as_bool(v)?,
            "zoom.step" => self.zoom_step = as_num(v, 1.01, 4.0)?,
            "zoom.default_fit" => {
                self.default_fit = match as_str(v)? {
                    "width" => FitMode::Width,
                    "page" => FitMode::Page,
                    "none" => FitMode::None,
                    other => {
                        return Err(format!(
                            "expected \"width\", \"page\" or \"none\", got \"{other}\""
                        ));
                    }
                }
            }
            "mouse.drag" => {
                self.drag = match as_str(v)? {
                    "select" => DragMode::Select,
                    "pan" => DragMode::Pan,
                    other => {
                        return Err(format!("expected \"select\" or \"pan\", got \"{other}\""));
                    }
                }
            }
            "mouse.pan_modifier" => self.pan_modifier = as_modifier(v)?,
            "mouse.zoom_modifier" => self.zoom_modifier = as_modifier(v)?,
            "mouse.copy_on_select" => self.copy_on_select = as_bool(v)?,
            _ => return Err("unknown option".into()),
        }
        Ok(())
    }

    /// `:set option value`. The option may be the full key ("zoom.step") or
    /// just its name when unambiguous ("toolbar"). The value is read as TOML
    /// (`1.2`, `true`, `"#ff0000"`), falling back to a plain string. Returns
    /// the full key that was set.
    pub fn set_from_str(&mut self, option: &str, value: &str) -> Result<&'static str, String> {
        let key = resolve_option(option)?;
        let parsed = format!("v = {value}")
            .parse::<Table>()
            .ok()
            .and_then(|mut t| t.remove("v"))
            .unwrap_or_else(|| Value::String(value.to_owned()));
        self.set(key, &parsed).map(|()| key)
    }

    /// The default normal-mode keymap with this config's overrides.
    pub fn normal_keymap(&self) -> (Keymap, Vec<String>) {
        let mut keymap = Keymap::default_normal();
        let mut warnings = Vec::new();
        for (seq, action) in &self.normal_keys {
            let result = parse_sequence(seq).and_then(|parsed| {
                if action.trim().is_empty() {
                    keymap.unbind(&parsed);
                    Ok(())
                } else {
                    Command::parse(action).map(|cmd| keymap.bind_parsed(parsed, cmd))
                }
            });
            if let Err(e) = result {
                warnings.push(format!("config: keys.normal.\"{seq}\": {e}"));
            }
        }
        (keymap, warnings)
    }
}

fn resolve_option(option: &str) -> Result<&'static str, String> {
    if let Some(key) = OPTIONS.iter().find(|k| **k == option) {
        return Ok(key);
    }
    let matches: Vec<&&str> = OPTIONS
        .iter()
        .filter(|k| k.rsplit('.').next() == Some(option))
        .collect();
    match matches.as_slice() {
        [key] => Ok(key),
        [] => Err(format!("unknown option '{option}'")),
        many => Err(format!(
            "'{option}' is ambiguous: {}",
            many.iter().map(|k| **k).collect::<Vec<_>>().join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colours() {
        assert_eq!(Color::parse("#ff8000"), Ok(Color([255, 128, 0, 255])));
        assert_eq!(
            Color::parse("#f9e2af80"),
            Ok(Color([0xf9, 0xe2, 0xaf, 0x80]))
        );
        assert!(Color::parse("ff8000").is_err());
        assert!(Color::parse("#ff80").is_err());
        assert!(Color::parse("#gg0000").is_err());
    }

    #[test]
    fn empty_config_gives_defaults() {
        let (c, w) = Config::from_toml("");
        assert_eq!(c, Config::default());
        assert!(w.is_empty());
    }

    #[test]
    fn loads_the_example_from_the_plan() {
        let text = r##"
[ui]
font_family = "Menlo"
font_size = 14
statusbar = true
start_maximised = true

[colors]
page_background = "#ffffff"
search_highlight = "#f9e2af80"

[scroll]
step = 60            # points per j/k
smooth = true

[zoom]
step = 1.2
default_fit = "page"

[keys.normal]
"<C-F>" = "toggle_fullscreen"
"x" = "scroll_down 3"
"q" = ""
"##;
        let (c, w) = Config::from_toml(text);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.zoom_step, 1.2);
        assert_eq!(c.default_fit, FitMode::Page);

        let (keymap, w) = c.normal_keymap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(keymap.get(&["x".to_owned()]), Some(&Command::ScrollDown(3)));
        assert_eq!(
            keymap.get(&["<C-F>".to_owned()]),
            Some(&Command::ToggleFullscreen)
        );
        assert_eq!(keymap.get(&["q".to_owned()]), None, "q was unbound");
    }

    #[test]
    fn invalid_entries_warn_and_keep_defaults() {
        let text = r##"
[ui]
font_size = "big"
nonsense = 1

[zoom]
step = 0.5
default_fit = "sideways"

[colors]
caret = "red"

[keys.normal]
"j" = "teleport"
"<Bogus>" = "quit"

[keys.visual]
"Y" = "yank_line"
"##;
        let (c, w) = Config::from_toml(text);
        assert_eq!(c.font_size, 14.0);
        assert_eq!(c.zoom_step, 1.1);
        assert_eq!(c.default_fit, FitMode::Width);
        assert_eq!(w.len(), 6, "{w:#?}");

        let (keymap, w) = c.normal_keymap();
        assert_eq!(w.len(), 2, "{w:#?}");
        assert_eq!(
            keymap.get(&["j".to_owned()]),
            Some(&Command::ScrollDown(1)),
            "bad binding keeps the default"
        );
    }

    #[test]
    fn example_config_is_valid_and_matches_defaults() {
        let text = include_str!("../config.example.toml");
        let (c, w) = Config::from_toml(text);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c, Config::default());
    }

    #[test]
    fn syntax_error_gives_defaults_and_one_warning() {
        let (c, w) = Config::from_toml("[ui\nfont_size = 3");
        assert_eq!(c, Config::default());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn set_accepts_short_names_and_toml_values() {
        let mut c = Config::default();
        assert_eq!(c.set_from_str("toolbar", "true"), Ok("ui.toolbar"));
        assert!(c.toolbar);
        assert_eq!(c.set_from_str("zoom.step", "1.25"), Ok("zoom.step"));
        assert_eq!(c.zoom_step, 1.25);
        assert_eq!(c.set_from_str("caret", "#00ff00"), Ok("colors.caret"));
        assert_eq!(c.colors.caret, Color([0, 255, 0, 255]));
        assert_eq!(
            c.set_from_str("default_fit", "page"),
            Ok("zoom.default_fit")
        );
        assert!(
            c.set_from_str("step", "2")
                .unwrap_err()
                .contains("ambiguous")
        );
        assert!(c.set_from_str("toolbar", "maybe").is_err());
        assert!(c.set_from_str("warp_speed", "9").is_err());
    }
}
