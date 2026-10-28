//! How finished frames are handed to the compositor (the wgpu present mode).
//!
//! On Linux vimpdf uses mailbox where the window supports it: with vsync
//! (FIFO), KDE Plasma on NVIDIA froze the whole desktop now and then, a few
//! seconds after a reload, until vimpdf was killed. Mailbox never makes the
//! compositor wait for vimpdf's frames. Support depends on the driver and the
//! window system (NVIDIA has it under Wayland but not X11), so it is asked of
//! the window's surface; the window starts with vsync, which every driver
//! has, and switches on its first frame. Elsewhere (macOS: Metal has no
//! mailbox) vsync stays. `VIMPDF_PRESENT_MODE` overrides the choice.

use eframe::SurfaceConfig;
use eframe::wgpu::{self, PresentMode};

const ENV: &str = "VIMPDF_PRESENT_MODE";

/// The mode named in `VIMPDF_PRESENT_MODE`, if any.
fn parse(value: &str) -> Option<PresentMode> {
    match value {
        "vsync" => Some(PresentMode::AutoVsync),
        "mailbox" => Some(PresentMode::Mailbox),
        "immediate" => Some(PresentMode::Immediate),
        "novsync" => Some(PresentMode::AutoNoVsync),
        _ => None,
    }
}

fn requested() -> Option<PresentMode> {
    let value = std::env::var(ENV).ok()?;
    let mode = parse(&value);
    if mode.is_none() {
        tracing::warn!("{ENV}: unknown value '{value}' (vsync, mailbox, immediate, novsync)");
    }
    mode
}

fn config(present_mode: PresentMode) -> SurfaceConfig {
    SurfaceConfig {
        present_mode,
        ..SurfaceConfig::LOW_LATENCY
    }
}

/// The surface config the window is created with: the one asked for in
/// `VIMPDF_PRESENT_MODE`, else vsync.
pub fn initial() -> SurfaceConfig {
    config(requested().unwrap_or(PresentMode::AutoVsync))
}

/// The mode to switch to once the window exists, given the modes its surface
/// supports: mailbox on Linux when available, unless the user chose a mode.
fn choose(linux: bool, overridden: bool, supported: &[PresentMode]) -> Option<PresentMode> {
    (linux && !overridden && supported.contains(&PresentMode::Mailbox))
        .then_some(PresentMode::Mailbox)
}

/// The config to switch to on the first frame, if any.
pub fn preferred(cc: &eframe::CreationContext<'_>) -> Option<SurfaceConfig> {
    let overridden = requested().is_some();
    let linux = cfg!(target_os = "linux");
    let supported = if linux && !overridden {
        supported_modes(cc)
    } else {
        Vec::new()
    };
    let choice = choose(linux, overridden, &supported);
    tracing::info!(
        initial = ?initial().present_mode,
        ?supported,
        switch_to = ?choice,
        "present mode"
    );
    choice.map(config)
}

/// The present modes of a throwaway surface for the window (the renderer's
/// own surface isn't reachable from here).
fn supported_modes(cc: &eframe::CreationContext<'_>) -> Vec<PresentMode> {
    let Some(rs) = &cc.wgpu_render_state else {
        return Vec::new();
    };
    // SAFETY: the window outlives the surface, which is dropped at the end
    // of this function.
    let surface = unsafe {
        wgpu::SurfaceTargetUnsafe::from_window(cc)
            .ok()
            .and_then(|target| rs.instance.create_surface_unsafe(target).ok())
    };
    surface.map_or_else(Vec::new, |s| s.get_capabilities(&rs.adapter).present_modes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_override() {
        assert_eq!(parse("mailbox"), Some(PresentMode::Mailbox));
        assert_eq!(parse("vsync"), Some(PresentMode::AutoVsync));
        assert_eq!(parse("novsync"), Some(PresentMode::AutoNoVsync));
        assert_eq!(parse("Mailbox"), None);
    }

    #[test]
    fn mailbox_only_on_linux_when_supported_and_not_overridden() {
        let with = [
            PresentMode::Fifo,
            PresentMode::Mailbox,
            PresentMode::Immediate,
        ];
        // NVIDIA under X11 has no mailbox.
        let without = [PresentMode::Fifo, PresentMode::Immediate];
        assert_eq!(choose(true, false, &with), Some(PresentMode::Mailbox));
        assert_eq!(choose(true, false, &without), None);
        assert_eq!(choose(false, false, &with), None, "not on macOS");
        assert_eq!(choose(true, true, &with), None, "the user's choice wins");
    }
}
