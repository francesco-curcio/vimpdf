//! Loading the configured UI font from the system font directories.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

const FONT_KEY: &str = "vimpdf-ui";

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/Library/Fonts"),
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];
    if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_owned()) {
        dirs.push(home.join("Library/Fonts"));
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join(".fonts"));
    }
    dirs
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Find the font file for `family`. `"monospace"` is the system's monospace
/// font (Menlo on macOS, fontconfig's choice elsewhere). Other names are
/// asked of fontconfig where it exists, then matched against file names
/// ("Menlo" → Menlo.ttc, "JetBrains Mono" → JetBrainsMono-Regular.ttf).
pub fn find_font(family: &str) -> Option<PathBuf> {
    let generic = normalize(family) == "monospace";
    if generic && cfg!(target_os = "macos") {
        return find_file("Menlo");
    }
    fontconfig(family, generic).or_else(|| find_file(family))
}

/// Ask `fc-match` for `family`. fc-match always answers with some font, so
/// unless the name is generic the answer must be the family asked for.
fn fontconfig(family: &str, generic: bool) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return None;
    }
    let out = std::process::Command::new("fc-match")
        .args(["--format", "%{family}\n%{file}", family])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let out = String::from_utf8_lossy(&out.stdout);
    let (families, file) = out.split_once('\n')?;
    let want = normalize(family);
    if !generic && !families.split(',').any(|f| normalize(f) == want) {
        return None;
    }
    let file = PathBuf::from(file.trim());
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    let usable = matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc");
    (usable && file.is_file()).then_some(file)
}

fn find_file(family: &str) -> Option<PathBuf> {
    let want = normalize(family);
    let mut best: Option<(u8, PathBuf)> = None;
    for dir in font_dirs() {
        search(&dir, 0, &want, &mut best);
    }
    best.map(|(_, p)| p)
}

fn search(dir: &Path, depth: u8, want: &str, best: &mut Option<(u8, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 4 {
                search(&path, depth + 1, want, best);
            }
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc") {
            continue;
        }
        let stem = normalize(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
        // Exact name beats "-Regular", which beats any other style.
        let rank = if stem == want {
            0
        } else if stem == format!("{want}regular") {
            1
        } else if stem.starts_with(want) {
            2
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            *best = Some((rank, path));
        }
    }
}

/// Make `family` the first monospace font. Returns an error message if it
/// can't be found (egui's built-in monospace font is used instead).
pub fn install(ctx: &egui::Context, family: &str) -> Result<(), String> {
    let mut defs = FontDefinitions::default();
    let path = find_font(family).ok_or_else(|| format!("font '{family}' not found"))?;
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    defs.font_data
        .insert(FONT_KEY.to_owned(), Arc::new(FontData::from_owned(bytes)));
    defs.families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, FONT_KEY.to_owned());
    ctx.set_fonts(defs);
    tracing::debug!(font = %path.display(), "installed UI font");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_family_names() {
        assert_eq!(normalize("JetBrains Mono"), "jetbrainsmono");
        assert_eq!(normalize("Menlo"), "menlo");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn finds_menlo_on_macos() {
        let path = find_font("Menlo").expect("Menlo ships with macOS");
        assert!(path.to_string_lossy().contains("Menlo"));
    }

    #[test]
    fn finds_a_monospace_font() {
        // Needs a font installed (always on macOS; fontconfig elsewhere).
        if cfg!(target_os = "macos") || which("fc-match") {
            assert!(find_font("monospace").is_some());
        }
    }

    #[test]
    fn missing_family_is_not_replaced_by_a_fallback() {
        assert!(find_font("No Such Font Family 42").is_none());
    }

    fn which(program: &str) -> bool {
        std::process::Command::new(program)
            .arg("--version")
            .output()
            .is_ok()
    }
}
