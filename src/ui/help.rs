use eframe::egui::{self, Align2, FontId, Grid, RichText, ScrollArea, Vec2};

use super::{ACCENT, BAR_DIM};
use crate::input::keymap::Keymap;

const MOUSE: &[(&str, &str)] = &[
    ("wheel / two-finger scroll", "scroll"),
    ("pinch, Cmd + wheel", "zoom at the pointer"),
    ("click a link", "follow it"),
    ("click page number in status bar", "go to page"),
];

const EX_COMMANDS: &[(&str, &str)] = &[
    (":<n>", "go to page n"),
    (":open <path>", "open another file"),
    (
        ":set <option> <value>",
        "change an option (e.g. :set toolbar)",
    ),
    (":source", "reload the config file"),
    (":help", "this overlay"),
    (":q", "quit"),
];

/// The `?` overlay, generated from the active keymap so config overrides show.
pub fn show(ctx: &egui::Context, keymap: &Keymap, open: &mut bool, font_size: f32) {
    let mono = FontId::monospace(font_size);
    let max = ctx.content_rect().size() * Vec2::new(0.8, 0.85);
    egui::Window::new("Keys")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .max_size(max)
        .show(ctx, |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                section(ui, "Keys", &mono);
                Grid::new("help-keys").striped(true).show(ui, |ui| {
                    for (keys, cmd) in keymap.describe() {
                        ui.label(RichText::new(keys).font(mono.clone()).color(ACCENT));
                        ui.label(RichText::new(cmd.describe()).font(mono.clone()));
                        ui.end_row();
                    }
                });
                ui.label(
                    RichText::new("Counts: 5j scrolls 5 steps, 12G goes to page 12.")
                        .color(BAR_DIM),
                );

                section(ui, "Mouse", &mono);
                pairs(ui, "help-mouse", MOUSE, &mono);
                section(ui, "Command line", &mono);
                pairs(ui, "help-ex", EX_COMMANDS, &mono);
                ui.add_space(4.0);
                ui.label(RichText::new("Esc or ? closes this.").color(BAR_DIM));
            });
        });
}

fn section(ui: &mut egui::Ui, title: &str, font: &FontId) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).font(font.clone()).strong());
}

fn pairs(ui: &mut egui::Ui, id: &str, rows: &[(&str, &str)], font: &FontId) {
    Grid::new(id).striped(true).show(ui, |ui| {
        for (a, b) in rows {
            ui.label(RichText::new(*a).font(font.clone()).color(ACCENT));
            ui.label(RichText::new(*b).font(font.clone()));
            ui.end_row();
        }
    });
}
