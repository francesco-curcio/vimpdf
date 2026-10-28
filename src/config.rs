//! Configuration: defaults, loading `~/.config/vimpdf/config.toml`, and `:set`.
//!
//! Every option goes through [`Config::set`], so the config file and `:set`
//! accept exactly the same values. Invalid entries never abort loading: they
//! produce a warning and the default is kept.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::input::command::Command;
use crate::input::keymap::{Keymap, parse_sequence};
use crate::viewport::FitMode;

/// The documented example config, used to create a config file on request.
pub const EXAMPLE: &str = include_str!("../config.example.toml");

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
    /// Start in recolor (dark) mode.
    pub recolor: bool,
    /// Reload the document when the file changes on disk.
    pub auto_reload: bool,
    /// Show the File / Edit / View menu bar.
    pub menubar: bool,
    pub colors: Colors,
    pub scroll_step: f32,
    pub smooth_scroll: bool,
    pub zoom_step: f32,
    pub default_fit: FitMode,
    pub drag: DragMode,
    pub pan_modifier: Modifier,
    pub zoom_modifier: Modifier,
    pub copy_on_select: bool,
    /// Highlight colours by letter (`a<letter>` in visual mode), in order.
    pub highlight_colors: Vec<(char, Color)>,
    /// Letter of the colour used by `c` (comment) and the first `aa`.
    pub highlight_default: char,
    /// Author written into new annotations; empty means the login name.
    pub author: String,
    /// Font size and colour of new text boxes.
    pub text_size: f32,
    pub text_color: Color,
    /// Key binding overrides, in file order.
    pub normal_keys: Vec<(String, String)>,
    pub visual_keys: Vec<(String, String)>,
}

impl Default for Config {
    fn default() -> Self {
        let c = |s| Color::parse(s).expect("valid default colour");
        Self {
            font_family: "monospace".into(),
            font_size: 14.0,
            statusbar: true,
            toolbar: false,
            start_maximised: true,
            recolor: false,
            auto_reload: true,
            menubar: true,
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
            // Sorted by letter, like a palette read from the config file.
            highlight_colors: [
                ('b', "#74c0fc"),
                ('g', "#8ce99a"),
                ('o', "#ffc078"),
                ('p', "#f783ac"),
                ('r', "#ff8787"),
                ('y', "#ffe066"),
            ]
            .into_iter()
            .map(|(k, v)| (k, c(v)))
            .collect(),
            highlight_default: 'y',
            author: String::new(),
            text_size: 12.0,
            text_color: c("#c92a2a"),
            normal_keys: Vec::new(),
            visual_keys: Vec::new(),
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
    "ui.recolor",
    "ui.auto_reload",
    "ui.menubar",
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
    "highlight.colors",
    "highlight.default",
    "highlight.author",
    "text.size",
    "text.color",
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
    /// The colour for a highlight letter.
    pub fn highlight_color(&self, letter: char) -> Option<Color> {
        self.highlight_colors
            .iter()
            .find(|(k, _)| *k == letter)
            .map(|(_, c)| *c)
    }

    /// The author name for new annotations.
    pub fn author_name(&self) -> String {
        if !self.author.is_empty() {
            return self.author.clone();
        }
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "vimpdf".into())
    }

    /// `$XDG_CONFIG_HOME/vimpdf/config.toml`, else `~/.config/vimpdf/config.toml`
    /// (also on macOS, to live next to other dotfiles).
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".config")))?;
        Some(base.join("vimpdf").join("config.toml"))
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
            let target = match mode.as_str() {
                "normal" => &mut self.normal_keys,
                "visual" => &mut self.visual_keys,
                _ => {
                    warnings.push(format!(
                        "config: keys.{mode}: no mode called '{mode}' (use normal or visual)"
                    ));
                    continue;
                }
            };
            for (seq, action) in bindings {
                match action.as_str() {
                    Some(a) => target.push((seq.clone(), a.to_owned())),
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
            "ui.recolor" => self.recolor = as_bool(v)?,
            "ui.auto_reload" => self.auto_reload = as_bool(v)?,
            "ui.menubar" => self.menubar = as_bool(v)?,
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
            "highlight.colors" => {
                let table = v.as_table().ok_or_else(|| {
                    format!("expected a table like {{ y = \"#ffe066\" }}, got {v}")
                })?;
                let mut colors = Vec::new();
                for (k, v) in table {
                    let mut chars = k.chars();
                    let letter = match (chars.next(), chars.next()) {
                        (Some(c), None) if c.is_ascii_lowercase() && c != 'a' => c,
                        _ => {
                            return Err(format!(
                                "'{k}' should be one lowercase letter other than 'a'"
                            ));
                        }
                    };
                    colors.push((letter, as_color(v)?));
                }
                colors.sort_by_key(|(k, _)| *k);
                if colors.is_empty() {
                    return Err("at least one colour is needed".into());
                }
                self.highlight_colors = colors;
            }
            "highlight.default" => {
                let s = as_str(v)?;
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => self.highlight_default = c,
                    _ => return Err(format!("expected one letter, got \"{s}\"")),
                }
            }
            "highlight.author" => self.author = as_str(v)?.to_owned(),
            "text.size" => self.text_size = as_num(v, 4.0, 72.0)?,
            "text.color" => self.text_color = as_color(v)?,
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
        apply_overrides(Keymap::default_normal(), &self.normal_keys, "normal")
    }

    /// The default visual-mode keymap with this config's overrides.
    pub fn visual_keymap(&self) -> (Keymap, Vec<String>) {
        apply_overrides(Keymap::default_visual(), &self.visual_keys, "visual")
    }
}

fn apply_overrides(
    mut keymap: Keymap,
    overrides: &[(String, String)],
    mode: &str,
) -> (Keymap, Vec<String>) {
    let mut warnings = Vec::new();
    for (seq, action) in overrides {
        let result = parse_sequence(seq).and_then(|parsed| {
            if action.trim().is_empty() {
                keymap.unbind(&parsed);
                Ok(())
            } else {
                Command::parse(action).map(|cmd| keymap.bind_parsed(parsed, cmd))
            }
        });
        if let Err(e) = result {
            warnings.push(format!("config: keys.{mode}.\"{seq}\": {e}"));
        }
    }
    (keymap, warnings)
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

[keys.insert]
"x" = "quit"
"##;
        let (c, w) = Config::from_toml(text);
        assert_eq!(c.font_size, 14.0);
        assert_eq!(c.zoom_step, 1.1);
        assert_eq!(c.default_fit, FitMode::Width);
        assert_eq!(w.len(), 6, "{w:#?}");

        let (keymap, w) = c.normal_keymap();
        assert_eq!(w.len(), 2, "{w:#?}");
        let (_, w) = c.visual_keymap();
        assert_eq!(w.len(), 1, "unknown command yank_line: {w:#?}");
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
    fn rebinding_changes_the_key_shown_in_menus() {
        let (c, _) = Config::from_toml(
            "[keys.normal]\n\"R\" = \"toggle_recolor\"\n\"<C-r>\" = \"\"\n\"<A-m>\" = \"menu\"",
        );
        let (km, w) = c.normal_keymap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(km.key_for(&Command::ToggleRecolor).as_deref(), Some("R"));
        assert_eq!(km.key_for(&Command::OpenMenu).as_deref(), Some("gm"));
    }

    #[test]
    fn highlight_palette() {
        let (c, w) = Config::from_toml(
            r##"
[highlight]
colors = { y = "#ffff00", x = "#123456" }
default = "x"
author = "Me"
"##,
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.highlight_color('x'), Some(Color([0x12, 0x34, 0x56, 255])));
        assert_eq!(c.highlight_color('g'), None);
        assert_eq!(c.highlight_default, 'x');
        assert_eq!(c.author_name(), "Me");

        let (c, w) = Config::from_toml("[highlight]\ncolors = { a = \"#ffffff\" }");
        assert_eq!(w.len(), 1, "'a' is reserved for the last colour");
        assert_eq!(c.highlight_colors.len(), 6);
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
