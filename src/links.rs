//! Links as plain data, loaded once on a background thread.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::document;

#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    /// 0-based page, with an optional position in page coordinates (points
    /// from the page's top-left).
    Internal {
        page: usize,
        x: Option<f32>,
        y: Option<f32>,
    },
    External(String),
}

/// A link area on a page, in points relative to the page's top-left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLink {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub target: LinkTarget,
}

impl PageLink {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
}

/// Load every page's links on a background thread. The receiver yields one
/// `Vec` per page, indexed by page number, once all pages are done.
pub fn spawn_loader(
    path: PathBuf,
    wake: impl Fn() + Send + 'static,
) -> Receiver<Vec<Vec<PageLink>>> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("pdfv-links".into())
        .spawn(move || {
            let start = std::time::Instant::now();
            match document::load_all_links(&path) {
                Ok(links) => {
                    let total: usize = links.iter().map(Vec::len).sum();
                    tracing::debug!(
                        total,
                        ms = start.elapsed().as_millis() as u64,
                        "loaded links"
                    );
                    if tx.send(links).is_ok() {
                        wake();
                    }
                }
                Err(err) => tracing::warn!("cannot load links: {err:#}"),
            }
        })
        .expect("failed to spawn link loader");
    rx
}

/// Characters used for hint labels, easiest to type first.
const HINT_CHARS: &[char] = &['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'];

/// `n` distinct hint labels, all the same length so none is a prefix of
/// another ("a".."l" for up to 9, then "aa", "as", …).
pub fn hint_labels(n: usize) -> Vec<String> {
    let base = HINT_CHARS.len();
    let mut len = 1;
    while base.pow(len as u32) < n {
        len += 1;
    }
    (0..n)
        .map(|mut i| {
            let mut label = vec![' '; len];
            for slot in label.iter_mut().rev() {
                *slot = HINT_CHARS[i % base];
                i /= base;
            }
            label.into_iter().collect()
        })
        .collect()
}

/// What to do with the chosen link in hint mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintAction {
    Follow,
    CopyAddress,
}

/// Open an external link in the default handler. Only web and mail links are
/// opened: a PDF could otherwise launch arbitrary local files or apps.
pub fn open_external(uri: &str) -> Result<(), String> {
    let lower = uri.to_ascii_lowercase();
    if !["http://", "https://", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return Err(format!("not opening non-web link: {uri}"));
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(uri)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot run {opener}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_labels_are_prefix_free() {
        assert_eq!(hint_labels(3), vec!["a", "s", "d"]);
        let many = hint_labels(30);
        assert_eq!(many.len(), 30);
        assert!(many.iter().all(|l| l.len() == 2));
        assert_eq!(&many[..2], &["aa", "as"]);
        let unique: std::collections::HashSet<_> = many.iter().collect();
        assert_eq!(unique.len(), 30);
        assert!(hint_labels(0).is_empty());
    }

    #[test]
    fn refuses_non_web_links() {
        assert!(open_external("file:///Applications/Calculator.app").is_err());
        assert!(open_external("javascript:alert(1)").is_err());
        assert!(open_external("/bin/sh").is_err());
    }
}
