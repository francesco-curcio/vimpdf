//! The comment editor: a small window with a multi-line text field.

use eframe::egui::{
    self, Align2, FontId, Id, Key, KeyboardShortcut, Modifiers, RichText, TextEdit, Vec2,
};

use super::BAR_DIM;

pub enum Outcome {
    Editing,
    Save(String),
    Cancel,
}

pub struct CommentEditor {
    pub text: String,
    title: String,
    needs_focus: bool,
}

impl CommentEditor {
    pub fn new(title: impl Into<String>, text: String) -> Self {
        Self {
            text,
            title: title.into(),
            needs_focus: true,
        }
    }

    /// Enter saves, Shift+Enter starts a new line, Esc cancels. (Modifier
    /// shortcuts like Ctrl+S don't reliably reach a focused text field on
    /// macOS, where the text system takes them.)
    ///
    /// Keys are ignored in the frame the editor opens: the Enter that
    /// confirmed placing a note or text box is still in that frame's input,
    /// and would save the empty text at once.
    pub fn show(&mut self, ctx: &egui::Context, font_size: f32) -> Outcome {
        let opening = self.needs_focus;
        let (save, cancel) = ctx.input(|i| {
            let plain_enter = i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. }
                        if !modifiers.shift
                )
            });
            (plain_enter, i.key_pressed(Key::Escape))
        });
        let (save, cancel) = (save && !opening, cancel && !opening);
        let id = Id::unique("vimpdf-comment-editor");
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        egui::Window::new(self.title.as_str())
            .collapsible(false)
            .resizable(true)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(420.0)
            .show(ctx, |ui| {
                let response = ui.add(
                    TextEdit::multiline(&mut self.text)
                        .id(id)
                        .font(FontId::proportional(font_size))
                        .desired_rows(6)
                        .desired_width(f32::INFINITY)
                        .hint_text("Comment…")
                        .return_key(Some(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))),
                );
                if self.needs_focus {
                    response.request_focus();
                    self.needs_focus = false;
                }
                ui.horizontal(|ui| {
                    save_clicked = ui.button("Save").clicked();
                    cancel_clicked = ui.button("Cancel").clicked();
                    ui.label(
                        RichText::new("Enter saves · Shift+Enter new line · Esc cancels")
                            .color(BAR_DIM),
                    );
                });
            });
        if save || save_clicked {
            Outcome::Save(self.text.trim().to_owned())
        } else if cancel || cancel_clicked {
            Outcome::Cancel
        } else {
            Outcome::Editing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, editor: &mut CommentEditor, events: Vec<egui::Event>) -> Outcome {
        let mut outcome = Outcome::Editing;
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        // Nothing is painted: drop the font atlas upload.
        ctx.run_pass(input, |ctx| outcome = editor.show(ctx, 14.0))
            .textures_delta
            .clear();
        outcome
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn the_enter_that_opened_it_does_not_save() {
        let ctx = egui::Context::default();
        let mut editor = CommentEditor::new("New note", String::new());
        assert!(matches!(
            frame(&ctx, &mut editor, vec![key(Key::Enter)]),
            Outcome::Editing
        ));
        assert!(matches!(
            frame(&ctx, &mut editor, vec![key(Key::Escape)]),
            Outcome::Cancel
        ));
    }

    #[test]
    fn typed_text_is_saved_with_enter() {
        let ctx = egui::Context::default();
        let mut editor = CommentEditor::new("New note", String::new());
        frame(&ctx, &mut editor, Vec::new());
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::Text("quick note".into())],
        );
        match frame(&ctx, &mut editor, vec![key(Key::Enter)]) {
            Outcome::Save(text) => assert_eq!(text, "quick note"),
            _ => panic!("expected Save"),
        }
    }
}
