use eframe::egui::{
    self, Align, Button, FontId, Id, Key, Layout, Rect, RichText, TextEdit, UiBuilder, Vec2,
};

use super::{BAR_BACKGROUND, BAR_DIM};
use crate::input::command::Command;

/// Text of the page-number and zoom fields while they are being edited.
#[derive(Default)]
pub struct Toolbar {
    page_text: String,
    zoom_text: String,
}

impl Toolbar {
    /// Draw the toolbar; buttons produce the same commands as their keys.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        page: usize,
        pages: usize,
        zoom_percent: f32,
        font_size: f32,
    ) -> Vec<Command> {
        ui.painter().rect_filled(rect, 0.0, BAR_BACKGROUND);
        let font = FontId::proportional(font_size);
        let mut commands = Vec::new();
        let mut bar = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(8.0, 3.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );

        if button(&mut bar, "−", "Zoom out (-)", &font) {
            commands.push(Command::ZoomOut(1));
        }
        // "125%": type a percentage and Enter to zoom there.
        let shown = format!("{zoom_percent:.0}%");
        if let Some(text) = field(
            &mut bar,
            &mut self.zoom_text,
            "vimpdf-toolbar-zoom",
            &shown,
            font_size * 3.5,
            font_size,
        ) {
            match text.trim_end_matches('%').trim().parse::<f32>() {
                Ok(p) if p > 0.0 => commands.push(Command::ZoomTo(p)),
                _ => self.zoom_text = shown,
            }
        }
        for (label, hint, cmd) in [
            ("+", "Zoom in (+)", Command::ZoomIn(1)),
            ("Fit width", "Fit width (s)", Command::FitWidth),
            ("Fit page", "Fit page (a)", Command::FitPage),
        ] {
            if button(&mut bar, label, hint, &font) {
                commands.push(cmd);
            }
        }
        bar.separator();

        let shown = page.to_string();
        if let Some(text) = field(
            &mut bar,
            &mut self.page_text,
            "vimpdf-toolbar-page",
            &shown,
            font_size * 3.0,
            font_size,
        ) {
            match text.trim().parse::<usize>() {
                Ok(n) => commands.push(Command::GotoPage(n)),
                Err(_) => self.page_text = shown,
            }
        }
        bar.label(
            RichText::new(format!("/ {pages}"))
                .font(FontId::monospace(font_size))
                .color(BAR_DIM),
        );

        bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if button(ui, "?", "Keys (?)", &font) {
                commands.push(Command::ToggleHelp);
            }
        });
        commands
    }
}

/// A small text field showing `shown` until it's focused. Returns the text
/// typed when Enter is pressed.
fn field(
    ui: &mut egui::Ui,
    text: &mut String,
    id: &str,
    shown: &str,
    width: f32,
    font_size: f32,
) -> Option<String> {
    let id = Id::unique(id);
    if !ui.memory(|m| m.has_focus(id)) {
        shown.clone_into(text);
    }
    let response = ui.add(
        TextEdit::singleline(text)
            .id(id)
            .font(FontId::monospace(font_size))
            .desired_width(width)
            .align(egui::Align2::CENTER_CENTER),
    );
    (response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))).then(|| text.clone())
}

fn button(ui: &mut egui::Ui, label: &str, hint: &str, font: &FontId) -> bool {
    ui.add(Button::new(RichText::new(label).font(font.clone())))
        .on_hover_text(hint)
        .clicked()
}

/// Height of the toolbar for a given font size.
pub fn height(font_size: f32) -> f32 {
    (font_size * 2.3).round()
}
