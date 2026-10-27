//! Per-file state remembered between sessions: position, zoom, fit mode and
//! marks. Stored in `$XDG_STATE_HOME/vimpdf/` (default `~/.local/state/vimpdf/`),
//! one small TOML file per document, named after a hash of its path.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml::{Table, Value};

use crate::jumplist::Position;
use crate::viewport::FitMode;

#[derive(Debug, Clone, PartialEq)]
pub struct FileState {
    pub position: Position,
    pub zoom: f32,
    pub fit: FitMode,
    pub marks: Vec<(char, Position)>,
}

pub fn state_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            directories::BaseDirs::new().map(|d| d.home_dir().join(".local").join("state"))
        })?;
    Some(base.join("vimpdf"))
}

/// FNV-1a: stable across Rust versions (unlike `DefaultHasher`), so state
/// files keep their names.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn file_for(dir: &Path, doc: &Path) -> PathBuf {
    let hash = fnv1a(doc.as_os_str().as_encoded_bytes());
    dir.join(format!("{hash:016x}.toml"))
}

fn fit_name(fit: FitMode) -> &'static str {
    match fit {
        FitMode::Width => "width",
        FitMode::Page => "page",
        FitMode::None => "none",
    }
}

fn position_value(p: Position) -> Value {
    let mut t = Table::new();
    t.insert("page".into(), Value::Integer(p.page as i64));
    t.insert("offset".into(), Value::Float(f64::from(p.offset)));
    t.insert("x".into(), Value::Float(f64::from(p.x)));
    Value::Table(t)
}

fn position_from(v: &Value) -> Option<Position> {
    let t = v.as_table()?;
    let num = |k: &str| {
        t.get(k)
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
    };
    Some(Position {
        page: usize::try_from(t.get("page")?.as_integer()?).ok()?,
        offset: num("offset")? as f32,
        x: num("x")? as f32,
    })
}

impl FileState {
    fn to_toml(&self, doc: &Path) -> String {
        let mut t = Table::new();
        t.insert("path".into(), Value::String(doc.display().to_string()));
        t.insert("position".into(), position_value(self.position));
        t.insert("zoom".into(), Value::Float(f64::from(self.zoom)));
        t.insert("fit".into(), Value::String(fit_name(self.fit).into()));
        let marks: Table = self
            .marks
            .iter()
            .map(|(c, p)| (c.to_string(), position_value(*p)))
            .collect();
        t.insert("marks".into(), Value::Table(marks));
        t.to_string()
    }

    fn from_toml(text: &str) -> Option<Self> {
        let t: Table = text.parse().ok()?;
        let fit = match t.get("fit")?.as_str()? {
            "width" => FitMode::Width,
            "page" => FitMode::Page,
            _ => FitMode::None,
        };
        let marks = t
            .get("marks")
            .and_then(Value::as_table)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| {
                        let mut chars = k.chars();
                        match (chars.next(), chars.next()) {
                            (Some(c), None) => Some((c, position_from(v)?)),
                            _ => None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            position: position_from(t.get("position")?)?,
            zoom: t.get("zoom")?.as_float()? as f32,
            fit,
            marks,
        })
    }
}

/// Recently read files (newest first) that still exist, from the state files.
pub fn recent(dir: &Path, limit: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            let table: Table = std::fs::read_to_string(e.path()).ok()?.parse().ok()?;
            let path = PathBuf::from(table.get("path")?.as_str()?);
            path.exists().then_some((modified, path))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, p)| p).take(limit).collect()
}

/// The saved state of `doc` (a canonical path), if any.
pub fn load(dir: &Path, doc: &Path) -> Option<FileState> {
    let text = std::fs::read_to_string(file_for(dir, doc)).ok()?;
    let state = FileState::from_toml(&text);
    if state.is_none() {
        tracing::warn!("ignoring unreadable state file for {}", doc.display());
    }
    state
}

pub fn save(dir: &Path, doc: &Path, state: &FileState) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let file = file_for(dir, doc);
    // Write then rename, so a crash never leaves half a file.
    let tmp = file.with_extension("toml.tmp");
    std::fs::write(&tmp, state.to_toml(doc))
        .with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, &file).with_context(|| format!("cannot write {}", file.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(page: usize, offset: f32) -> Position {
        Position {
            page,
            offset,
            x: -3.5,
        }
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("vimpdf-state-test-{}", std::process::id()));
        let doc = Path::new("/papers/attention.pdf");
        assert_eq!(load(&dir, doc), None);

        let state = FileState {
            position: pos(12, 240.5),
            zoom: 1.75,
            fit: FitMode::Page,
            marks: vec![('\'', pos(1, 10.0)), ('a', pos(3, 0.0))],
        };
        save(&dir, doc, &state).unwrap();
        assert_eq!(load(&dir, doc), Some(state));
        assert_eq!(load(&dir, Path::new("/papers/other.pdf")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hash_is_stable() {
        // Changing this would orphan everyone's saved positions.
        assert_eq!(fnv1a(b"/papers/attention.pdf"), 0xb009_4b99_6415_cf58);
    }
}
