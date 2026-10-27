use eframe::egui::{
    self, Align, CursorIcon, FontId, Label, Layout, Rect, RichText, Sense, UiBuilder, Vec2,
};

use super::{ACCENT, BAR_BACKGROUND, BAR_DIM, BAR_TEXT, ERROR};
use crate::input::command::Command;

pub struct Status<'a> {
    pub mode: &'a str,
    pub file: &'a str,
    /// 1-based.
    pub page: usize,
    pub pages: usize,
    pub zoom_percent: f32,
    /// Count / partial key sequence being typed.
    pub pending: String,
    /// Transient message, and whether it's an error.
    pub message: Option<(&'a str, bool)>,
    pub font_size: f32,
}

/// Draw the status bar. Clicking the page number opens the command line.
pub fn show(ui: &mut egui::Ui, rect: Rect, s: &Status) -> Option<Command> {
    ui.painter().rect_filled(rect, 0.0, BAR_BACKGROUND);
    let font = FontId::monospace(s.font_size);
    let text = |t: &str| RichText::new(t).font(font.clone()).color(BAR_TEXT);

    let mut command = None;
    let mut bar = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(8.0, 0.0)))
            .layout(Layout::right_to_left(Align::Center)),
    );
    bar.spacing_mut().item_spacing.x = s.font_size;

    // Right side first, so a long file name is truncated rather than
    // pushing the page number off screen.
    bar.label(text(&format!("{:.0}%", s.zoom_percent)));
    let page = bar
        .add(Label::new(text(&format!("{}/{}", s.page, s.pages))).sense(Sense::click()))
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Go to page (:)");
    if page.clicked() {
        command = Some(Command::EnterCommandLine(String::new()));
    }
    if !s.pending.is_empty() {
        bar.label(RichText::new(&s.pending).font(font.clone()).color(ACCENT));
    }

    bar.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.label(RichText::new(s.mode).font(font.clone()).color(ACCENT));
        let (msg, color) = match s.message {
            Some((m, true)) => (m, ERROR),
            Some((m, false)) => (m, BAR_TEXT),
            None => (s.file, BAR_DIM),
        };
        ui.add(Label::new(RichText::new(msg).font(font.clone()).color(color)).truncate());
    });
    command
}
