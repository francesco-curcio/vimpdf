use eframe::egui::{
    self, Align, Button, FontId, Id, Key, Layout, Rect, RichText, TextEdit, UiBuilder, Vec2,
};

use super::{BAR_BACKGROUND, BAR_DIM};
use crate::input::command::Command;

/// Text of the page-number field while it is being edited.
#[derive(Default)]
pub struct Toolbar {
    page_text: String,
}

impl Toolbar {
    /// Draw the toolbar; buttons produce the same commands as their keys.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        page: usize,
        pages: usize,
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

        for (label, hint, cmd) in [
            ("−", "Zoom out (-)", Command::ZoomOut(1)),
            ("+", "Zoom in (+)", Command::ZoomIn(1)),
            ("Fit width", "Fit width (s)", Command::FitWidth),
            ("Fit page", "Fit page (a)", Command::FitPage),
        ] {
            if button(&mut bar, label, hint, &font) {
                commands.push(cmd);
            }
        }
        bar.separator();

        let id = Id::unique("pdfv-toolbar-page");
        let focused = bar.memory(|m| m.has_focus(id));
        if !focused {
            self.page_text = page.to_string();
        }
        let response = bar.add(
            TextEdit::singleline(&mut self.page_text)
                .id(id)
                .font(FontId::monospace(font_size))
                .desired_width(font_size * 3.0)
                .align(egui::Align2::CENTER_CENTER),
        );
        if response.lost_focus() && bar.input(|i| i.key_pressed(Key::Enter)) {
            match self.page_text.trim().parse::<usize>() {
                Ok(n) => commands.push(Command::GotoPage(n)),
                Err(_) => self.page_text = page.to_string(),
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

fn button(ui: &mut egui::Ui, label: &str, hint: &str, font: &FontId) -> bool {
    ui.add(Button::new(RichText::new(label).font(font.clone())))
        .on_hover_text(hint)
        .clicked()
}

/// Height of the toolbar for a given font size.
pub fn height(font_size: f32) -> f32 {
    (font_size * 2.3).round()
}
