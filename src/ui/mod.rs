//! egui widgets around the document: status bar, command line, toolbar and
//! the help overlay. They report user actions as `Command`s.

pub mod annotations;
pub mod cmdline;
pub mod comment;
pub mod fonts;
pub mod help;
pub mod menubar;
pub mod outline;
pub mod statusbar;
pub mod toolbar;

use eframe::egui::Color32;

pub const BAR_BACKGROUND: Color32 = Color32::from_gray(24);
pub const BAR_TEXT: Color32 = Color32::from_gray(210);
pub const BAR_DIM: Color32 = Color32::from_gray(140);
pub const ACCENT: Color32 = Color32::from_rgb(137, 180, 250);
pub const ERROR: Color32 = Color32::from_rgb(243, 139, 168);

/// Height of the status bar / command line for a given font size.
pub fn bar_height(font_size: f32) -> f32 {
    (font_size * 1.7).round()
}
