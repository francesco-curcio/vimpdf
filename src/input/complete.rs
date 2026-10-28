//! Tab completion for the `:` command line: command names, `:set` options,
//! and file paths after the commands that take one.

use std::path::{Path, PathBuf};

use super::command::{EX_COMMANDS, PATH_COMMANDS, command_names};
use crate::config::OPTIONS;

/// What Tab can complete: the text from byte `start` on is replaced by one
/// of `candidates`.
#[derive(Debug, Default, PartialEq)]
pub struct Completion {
    pub start: usize,
    pub candidates: Vec<String>,
}

/// Completions for the end of `line`. Relative paths are relative to
/// `base_dir` (the open document's folder, like `:open` and `:w`).
pub fn complete(line: &str, base_dir: &Path) -> Completion {
    let lead = line.len() - line.trim_start().len();
    let rest = &line[lead..];
    let Some((word, arg)) = rest.split_once(char::is_whitespace) else {
        // The command name itself.
        let mut candidates: Vec<String> = EX_COMMANDS
            .iter()
            .copied()
            .chain(command_names())
            .filter(|n| n.starts_with(rest))
            .map(str::to_owned)
            .collect();
        candidates.sort();
        candidates.dedup();
        return Completion {
            start: lead,
            candidates,
        };
    };
    let arg_start = line.len() - arg.trim_start().len();
    let arg = arg.trim_start();
    let candidates = if word == "set" || word == "se" {
        options(arg)
    } else if PATH_COMMANDS.contains(&word) {
        paths(arg, base_dir)
    } else {
        Vec::new()
    };
    Completion {
        start: arg_start,
        candidates,
    }
}

/// Options whose full name ("ui.toolbar") or short name ("toolbar") starts
/// with `typed`.
fn options(typed: &str) -> Vec<String> {
    OPTIONS
        .iter()
        .filter(|o| {
            o.starts_with(typed) || o.split_once('.').is_some_and(|(_, n)| n.starts_with(typed))
        })
        .map(|o| (*o).to_owned())
        .collect()
}

/// Folders and PDFs whose path starts with `typed`. Folders end in `/` so
/// the next Tab goes inside. Hidden entries only when the name typed starts
/// with a dot.
fn paths(typed: &str, base_dir: &Path) -> Vec<String> {
    let (dir_part, name) = match typed.rfind('/') {
        Some(i) => typed.split_at(i + 1),
        None => ("", typed),
    };
    let dir: PathBuf = if dir_part == "~/" || dir_part.starts_with("~/") {
        match directories::BaseDirs::new() {
            Some(d) => d.home_dir().join(&dir_part[2..]),
            None => return Vec::new(),
        }
    } else if dir_part.is_empty() {
        base_dir.to_owned()
    } else {
        base_dir.join(dir_part)
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let file_name = e.file_name().to_string_lossy().into_owned();
            if !file_name.starts_with(name)
                || (file_name.starts_with('.') && !name.starts_with('.'))
            {
                return None;
            }
            // Follows symlinks: a link to a folder is a folder.
            let is_dir = e.path().is_dir();
            let is_pdf = Path::new(&file_name)
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("pdf"));
            if is_dir {
                Some(format!("{dir_part}{file_name}/"))
            } else if is_pdf {
                Some(format!("{dir_part}{file_name}"))
            } else {
                None
            }
        })
        .collect();
    out.sort();
    out
}

/// The longest prefix all `candidates` share (whole characters).
pub fn common_prefix(candidates: &[String]) -> &str {
    let Some(first) = candidates.first() else {
        return "";
    };
    let mut end = first.len();
    for c in &candidates[1..] {
        end = first
            .char_indices()
            .zip(c.chars())
            .find(|((_, a), b)| a != b)
            .map_or(end.min(c.len()), |((i, _), _)| i.min(end));
    }
    &first[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vimpdf-complete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["papers/old", "photos", ".hidden"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in [
            "paper.pdf",
            "papers/a.pdf",
            "papers/b.PDF",
            "papers/notes.txt",
            "photos/x.pdf",
            ".secret.pdf",
        ] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        dir
    }

    #[test]
    fn command_names() {
        let c = complete("sav", Path::new("/"));
        assert_eq!(c.start, 0);
        assert_eq!(c.candidates, vec!["saveas"]);
        let c = complete("  redact", Path::new("/"));
        assert_eq!(c.start, 2);
        assert_eq!(
            c.candidates,
            vec!["redact", "redact-matches", "redact_area", "redact_matches"]
        );
        assert!(
            complete("toggle_t", Path::new("/"))
                .candidates
                .contains(&"toggle_toolbar".into())
        );
        assert!(complete("zzz", Path::new("/")).candidates.is_empty());
    }

    #[test]
    fn set_options_by_full_or_short_name() {
        let c = complete("set tool", Path::new("/"));
        assert_eq!(c.start, 4);
        assert_eq!(c.candidates, vec!["ui.toolbar"]);
        assert!(complete("set zoom.", Path::new("/")).candidates.len() >= 2);
    }

    #[test]
    fn paths_relative_to_the_document_folder() {
        let dir = tree();
        let c = complete("open pa", &dir);
        assert_eq!(c.start, 5);
        assert_eq!(c.candidates, vec!["paper.pdf", "papers/"]);
        // Inside a folder: PDFs (any case) and folders, not other files.
        let c = complete("w papers/", &dir);
        assert_eq!(c.start, 2);
        assert_eq!(
            c.candidates,
            vec!["papers/a.pdf", "papers/b.PDF", "papers/old/"]
        );
        // Hidden entries only when asked for.
        assert!(
            !complete("e ", &dir)
                .candidates
                .iter()
                .any(|c| c.starts_with('.'))
        );
        assert_eq!(
            complete("e .", &dir).candidates,
            vec![".hidden/", ".secret.pdf"]
        );
        // Absolute paths ignore the document folder.
        let abs = format!("saveas {}/pho", dir.display());
        assert_eq!(
            complete(&abs, Path::new("/nowhere")).candidates,
            vec![format!("{}/photos/", dir.display())]
        );
        // Commands without a path argument complete nothing.
        assert!(complete("goto_page 1", &dir).candidates.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn common_prefix_of_candidates() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            common_prefix(&v(&["papers/a.pdf", "papers/b.pdf"])),
            "papers/"
        );
        assert_eq!(common_prefix(&v(&["paper.pdf", "papers/"])), "paper");
        assert_eq!(common_prefix(&v(&["only"])), "only");
        assert_eq!(common_prefix(&v(&[])), "");
        assert_eq!(common_prefix(&v(&["àb", "àc"])), "à");
    }
}
