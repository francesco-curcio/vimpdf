//! Watching the open file for changes (e.g. a LaTeX recompile).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    changes: Receiver<()>,
}

impl FileWatcher {
    /// Watch `path`. The parent directory is watched (not the file), because
    /// many tools replace a file by writing a new one and renaming it.
    pub fn new(path: &Path, wake: impl Fn() + Send + 'static) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let name = path.file_name().map(|n| n.to_owned());
        let dir: PathBuf = path.parent().unwrap_or(Path::new(".")).to_owned();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let Ok(event) = res else { return };
                let relevant = matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) && event
                    .paths
                    .iter()
                    .any(|p| p.file_name().map(|n| n.to_owned()) == name);
                if relevant && tx.send(()).is_ok() {
                    wake();
                }
            })?;
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        Ok(Self {
            _watcher: watcher,
            changes: rx,
        })
    }

    /// Whether the file changed since the last call.
    pub fn changed(&self) -> bool {
        let mut any = false;
        while self.changes.try_recv().is_ok() {
            any = true;
        }
        any
    }
}
