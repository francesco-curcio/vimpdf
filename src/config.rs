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

    /// "#rrggbb", or "#rrggbbaa" when not opaque.
    pub fn to_hex(self) -> String {
        let [r, g, b, a] = self.0;
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
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
    /// Recolor keeps colours' hue instead of making the page monochrome.
    pub recolor_keep_hue: bool,
    /// Reload the document when the file changes on disk.
    pub auto_reload: bool,
    /// Show the File / Edit / View menu bar.
    pub menubar: bool,
    /// Show the tab bar.
    pub tabbar: bool,
    /// Show the scrollbar beside the pages.
    pub scrollbar: bool,
    /// Files opened while vimpdf runs become tabs of the running window.
    pub single_instance: bool,
    pub colors: Colors,
    pub scroll_step: f32,
    pub smooth_scroll: bool,
    pub zoom_step: f32,
    pub default_fit: FitMode,
    pub drag: DragMode,
    pub pan_modifier: Modifier,
    pub zoom_modifier: Modifier,
    pub copy_on_select: bool,
    /// Hide the mouse pointer over the pages after this many idle seconds;
    /// 0 never hides it.
    pub hide_pointer_after: f32,
    /// Keep the outline open beside the pages.
    pub outline_pinned: bool,
    /// Width of the outline panel, in points.
    pub outline_width: f32,
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
            recolor_keep_hue: true,
            auto_reload: true,
            menubar: true,
            tabbar: true,
            scrollbar: true,
            single_instance: true,
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
            hide_pointer_after: 2.0,
            outline_pinned: false,
            outline_width: 320.0,
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

/// Narrowest the outline panel gets.
pub const OUTLINE_MIN_WIDTH: f32 = 160.0;

/// Every option, as "section.name".
pub const OPTIONS: &[&str] = &[
    "ui.font_family",
    "ui.font_size",
    "ui.statusbar",
    "ui.toolbar",
    "ui.start_maximised",
    "ui.recolor",
    "ui.recolor_keep_hue",
    "ui.auto_reload",
    "ui.menubar",
    "ui.tabbar",
    "ui.scrollbar",
    "ui.single_instance",
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
    "mouse.hide_after",
    "outline.pinned",
    "outline.width",
    "highlight.colors",
    "highlight.default",
    "highlight.author",
    "text.size",
    "text.color",
];

/// Write option values (full key, value) into the config file at `path`,
/// creating it if needed. Everything else in the file stays as it was,
/// comments included; a changed option keeps its trailing comment.
pub fn save_options(path: &Path, values: &[(&str, Value)]) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        let msg = e.to_string();
        let first = msg.lines().next().unwrap_or("syntax error").to_owned();
        format!("{} can't be updated: {first}", path.display())
    })?;
    for (key, value) in values {
        let Some((section, name)) = key.split_once('.') else {
            return Err(format!("not an option: {key}"));
        };
        let table = doc
            .entry(section)
            .or_insert_with(toml_edit::table)
            .as_table_like_mut()
            .ok_or_else(|| format!("'{section}' in the config file is not a [section]"))?;
        let new = edit_value(value);
        match table.get_mut(name).and_then(|i| i.as_value_mut()) {
            Some(old) => {
                let decor = old.decor().clone();
                *old = new;
                *old.decor_mut() = decor;
            }
            None => {
                table.insert(name, toml_edit::Item::Value(new));
            }
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    // Write next to it and rename, so a failed write never truncates it.
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string())
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn edit_value(v: &Value) -> toml_edit::Value {
    match v {
        Value::Boolean(b) => (*b).into(),
        Value::Integer(i) => (*i).into(),
        Value::Float(f) => (*f).into(),
        Value::Table(t) => {
            let mut inline = toml_edit::InlineTable::new();
            for (k, v) in t {
                inline.insert(k, edit_value(v));
            }
            inline.fmt();
            inline.into()
        }
        other => other.as_str().unwrap_or_default().into(),
    }
}

/// A number as written in the config: whole numbers without ".0" (a
/// float rounded to 4 decimals otherwise, so 1.1f32 is written 1.1).
fn num(v: f32) -> Value {
    if v.fract() == 0.0 && v.abs() < 1e9 {
        Value::Integer(v as i64)
    } else {
        Value::Float((f64::from(v) * 10_000.0).round() / 10_000.0)
    }
}

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
            "ui.recolor_keep_hue" => self.recolor_keep_hue = as_bool(v)?,
            "ui.auto_reload" => self.auto_reload = as_bool(v)?,
            "ui.menubar" => self.menubar = as_bool(v)?,
            "ui.tabbar" => self.tabbar = as_bool(v)?,
            "ui.scrollbar" => self.scrollbar = as_bool(v)?,
            "ui.single_instance" => self.single_instance = as_bool(v)?,
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
            "mouse.hide_after" => self.hide_pointer_after = as_num(v, 0.0, 3600.0)?,
            "outline.pinned" => self.outline_pinned = as_bool(v)?,
            "outline.width" => {
                self.outline_width = as_num(v, OUTLINE_MIN_WIDTH as f64, 2000.0)?;
            }
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

    /// The value of one option, by its full key, as it would be written in
    /// the config file. `None` for an unknown key.
    pub fn get(&self, key: &str) -> Option<Value> {
        let b = |v: bool| Value::Boolean(v);
        let s = |v: &str| Value::String(v.to_owned());
        let c = |v: Color| Value::String(v.to_hex());
        let m = |v: Modifier| {
            s(match v {
                Modifier::Cmd => "cmd",
                Modifier::Ctrl => "ctrl",
                Modifier::Alt => "alt",
                Modifier::Shift => "shift",
            })
        };
        Some(match key {
            "ui.font_family" => s(&self.font_family),
            "ui.font_size" => num(self.font_size),
            "ui.statusbar" => b(self.statusbar),
            "ui.toolbar" => b(self.toolbar),
            "ui.start_maximised" => b(self.start_maximised),
            "ui.recolor" => b(self.recolor),
            "ui.recolor_keep_hue" => b(self.recolor_keep_hue),
            "ui.auto_reload" => b(self.auto_reload),
            "ui.menubar" => b(self.menubar),
            "ui.tabbar" => b(self.tabbar),
            "ui.scrollbar" => b(self.scrollbar),
            "ui.single_instance" => b(self.single_instance),
            "colors.page_background" => c(self.colors.page_background),
            "colors.recolor_dark" => c(self.colors.recolor_dark),
            "colors.recolor_light" => c(self.colors.recolor_light),
            "colors.search_highlight" => c(self.colors.search_highlight),
            "colors.selection" => c(self.colors.selection),
            "colors.caret" => c(self.colors.caret),
            "colors.hint_background" => c(self.colors.hint_background),
            "scroll.step" => num(self.scroll_step),
            "scroll.smooth" => b(self.smooth_scroll),
            "zoom.step" => num(self.zoom_step),
            "zoom.default_fit" => s(match self.default_fit {
                FitMode::Width => "width",
                FitMode::Page => "page",
                FitMode::None => "none",
            }),
            "mouse.drag" => s(match self.drag {
                DragMode::Select => "select",
                DragMode::Pan => "pan",
            }),
            "mouse.pan_modifier" => m(self.pan_modifier),
            "mouse.zoom_modifier" => m(self.zoom_modifier),
            "mouse.copy_on_select" => b(self.copy_on_select),
            "mouse.hide_after" => num(self.hide_pointer_after),
            "outline.pinned" => b(self.outline_pinned),
            "outline.width" => num(self.outline_width),
            "highlight.colors" => Value::Table(
                self.highlight_colors
                    .iter()
                    .map(|(k, v)| (k.to_string(), c(*v)))
                    .collect(),
            ),
            "highlight.default" => s(&self.highlight_default.to_string()),
            "highlight.author" => s(&self.author),
            "text.size" => num(self.text_size),
            "text.color" => c(self.text_color),
            _ => return None,
        })
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
    fn every_option_reads_back_what_it_was_set_to() {
        let mut c = Config::default();
        for key in OPTIONS {
            let v = c.get(key).unwrap_or_else(|| panic!("{key} has no getter"));
            let before = c.clone();
            assert_eq!(c.set(key, &v), Ok(()), "{key} = {v}");
            assert_eq!(c, before, "{key} changed when set to its own value");
        }
        assert_eq!(c.get("nope.nothing"), None);
        assert_eq!(c.get("zoom.step"), Some(Value::Float(1.1)));
        assert_eq!(c.get("ui.font_size"), Some(Value::Integer(14)));
        assert_eq!(
            c.get("colors.selection"),
            Some(Value::String("#89b4fa60".into()))
        );
    }

    #[test]
    fn saving_options_keeps_the_rest_of_the_file() {
        let dir = std::env::temp_dir().join(format!("vimpdf-save-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);

        // No file yet: created with just these.
        save_options(&path, &[("ui.toolbar", Value::Boolean(true))]).unwrap();
        assert!(Config::load(&path).0.toolbar);

        std::fs::write(
            &path,
            "# my config\n[ui]\nfont_size = 14   # bigger later\nmenubar = false\n\n\
             [keys.normal]\n\"x\" = \"quit\"\n",
        )
        .unwrap();
        let colours: Table = [("y".to_owned(), Value::String("#ffff00".into()))]
            .into_iter()
            .collect();
        save_options(
            &path,
            &[
                ("ui.font_size", Value::Integer(16)),
                ("zoom.step", Value::Float(1.25)),
                ("highlight.colors", Value::Table(colours)),
            ],
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# my config\n[ui]\n"), "{text}");
        assert!(text.contains("font_size = 16   # bigger later"), "{text}");
        assert!(text.contains("menubar = false"), "{text}");
        assert!(text.contains("\"x\" = \"quit\""), "{text}");
        let (c, warnings) = Config::load(&path);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!((c.font_size, c.zoom_step), (16.0, 1.25));
        assert_eq!(c.highlight_colors.len(), 1);
        assert!(!c.menubar);

        std::fs::write(&path, "[ui\n").unwrap();
        assert!(save_options(&path, &[("ui.toolbar", Value::Boolean(true))]).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[ui\n",
            "left alone"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
