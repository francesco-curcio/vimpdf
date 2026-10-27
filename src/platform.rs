//! Files opened from outside the app.
//!
//! On macOS, double-clicking a PDF, "Open With", dropping on the Dock icon or
//! a browser handing over a download don't pass a command-line argument:
//! AppKit sends `application:openURLs:` to the application delegate. winit
//! leaves the delegate to us, so we install one that queues the paths for
//! the UI. Files opened while launching arrive before the window is created.

use std::path::PathBuf;
use std::sync::Mutex;

type Waker = Box<dyn Fn() + Send>;

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static WAKE: Mutex<Option<Waker>> = Mutex::new(None);

/// Whether `path` is in a temporary folder (where browsers put files they
/// open with another app, and may delete them later).
pub fn is_temporary(path: &std::path::Path) -> bool {
    let temp = std::env::temp_dir();
    let temp = std::fs::canonicalize(&temp).unwrap_or(temp);
    path.starts_with(&temp)
        || [
            "/tmp/",
            "/private/tmp/",
            "/var/folders/",
            "/private/var/folders/",
            "/var/tmp/",
        ]
        .iter()
        .any(|p| path.starts_with(p))
}

/// Paths opened since the last call, oldest first.
pub fn take_opened() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().expect("lock"))
}

/// Called when new paths arrive (to repaint the UI).
pub fn set_waker(wake: impl Fn() + Send + 'static) {
    *WAKE.lock().expect("lock") = Some(Box::new(wake));
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn push_opened(paths: impl IntoIterator<Item = PathBuf>) {
    OPENED.lock().expect("lock").extend(paths);
    if let Some(wake) = WAKE.lock().expect("lock").as_ref() {
        wake();
    }
}

/// Install the handler. Must run on the main thread, after the winit event
/// loop is created and before it runs.
#[cfg(target_os = "macos")]
pub fn install_open_handler() {
    macos::install();
}

#[cfg(not(target_os = "macos"))]
pub fn install_open_handler() {}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use objc2::runtime::{AnyObject, Imp, ProtocolObject, Sel};
    use objc2::{MainThreadMarker, ffi, sel};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSArray, NSURL};

    /// `-[delegate application:openURLs:]`
    unsafe extern "C-unwind" fn open_urls(
        _this: &AnyObject,
        _cmd: Sel,
        _app: &NSApplication,
        urls: &NSArray<NSURL>,
    ) {
        let paths: Vec<PathBuf> = urls
            .iter()
            .filter_map(|url| url.path())
            .map(|p| PathBuf::from(p.to_string()))
            .collect();
        tracing::debug!(?paths, "opened from the system");
        super::push_opened(paths);
    }

    /// winit 0.30 installs its own application delegate (and aborts if it is
    /// replaced), so instead of setting ours we add the method to its class.
    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else {
            tracing::warn!("not on the main thread: files opened from Finder are ignored");
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let Some(delegate) = app.delegate() else {
            tracing::warn!("no application delegate: files opened from Finder are ignored");
            return;
        };
        let object: &AnyObject = ProtocolObject::as_ref(&delegate);
        let class = object.class();
        // SAFETY: the function has the signature the type encoding describes
        // (void return; self, _cmd, two objects), which is AppKit's for this
        // selector. Imp is the runtime's untyped function pointer.
        let added = unsafe {
            let imp: Imp = std::mem::transmute(
                open_urls
                    as unsafe extern "C-unwind" fn(
                        &AnyObject,
                        Sel,
                        &NSApplication,
                        &NSArray<NSURL>,
                    ),
            );
            ffi::class_addMethod(
                (class as *const objc2::runtime::AnyClass).cast_mut(),
                sel!(application:openURLs:),
                imp,
                c"v@:@@".as_ptr(),
            )
        };
        if added.as_bool() {
            tracing::debug!(class = %class.name().to_string_lossy(), "installed open-URLs handler");
        } else {
            tracing::warn!("the application delegate already handles opened files");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_paths() {
        let t = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        assert!(is_temporary(&t.join("mozilla_x/paper.pdf")));
        assert!(is_temporary(std::path::Path::new("/tmp/paper.pdf")));
        assert!(!is_temporary(std::path::Path::new(
            "/Users/me/Papers/paper.pdf"
        )));
        assert!(!is_temporary(std::path::Path::new(
            "/home/me/Downloads/paper.pdf"
        )));
    }
}
