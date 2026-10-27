use eframe::egui::{self, Align2, FontId, Grid, RichText, ScrollArea, Vec2};

use super::{ACCENT, BAR_DIM};
use crate::input::keymap::Keymap;

const MOUSE: &[(&str, &str)] = &[
    ("wheel / two-finger scroll", "scroll (j k h l)"),
    ("pinch, Cmd + wheel", "zoom at the pointer (+ -)"),
    ("click a link", "follow it (f)"),
    ("drag over text", "select (v + motions)"),
    ("double / triple click", "select word / line (gv, V)"),
    ("Shift + click", "extend the selection"),
    ("Alt + drag, middle drag", "pan (h j k l)"),
    ("right click", "menu: copy (y), search (*), link (f, yf)"),
    ("click page number in status bar", "go to page (:<n>)"),
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

/// Scroll position of the help overlay, driven by the keyboard.
#[derive(Debug, Default)]
pub struct HelpScroll {
    offset: f32,
    /// Offset to apply on the next frame (after a key).
    requested: Option<f32>,
    /// Visible height and largest offset, from the last frame.
    view_height: f32,
    max: f32,
}

impl HelpScroll {
    pub fn by(&mut self, dy: f32) {
        self.requested = Some((self.offset + dy).clamp(0.0, self.max));
    }

    pub fn scroll_top(&mut self) {
        self.requested = Some(0.0);
    }

    pub fn scroll_bottom(&mut self) {
        self.requested = Some(self.max);
    }

    pub fn requested(&self) -> Option<f32> {
        self.requested
    }

    pub fn view_height(&self) -> f32 {
        self.view_height
    }
}

/// The `?` overlay, generated from the active keymap so config overrides show.
pub fn show(
    ctx: &egui::Context,
    keymap: &Keymap,
    visual: &Keymap,
    open: &mut bool,
    scroll: &mut HelpScroll,
    font_size: f32,
) {
    let mono = FontId::monospace(font_size);
    let max = ctx.content_rect().size() * Vec2::new(0.8, 0.85);
    egui::Window::new("Keys")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .max_size(max)
        .show(ctx, |ui| {
            let mut area = ScrollArea::vertical();
            if let Some(offset) = scroll.requested.take() {
                area = area.vertical_scroll_offset(offset);
            }
            let out = area.show(ui, |ui| {
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

                section(ui, "Visual mode (v, V, gv, or a mouse selection)", &mono);
                Grid::new("help-visual").striped(true).show(ui, |ui| {
                    for (keys, cmd) in visual.describe() {
                        ui.label(RichText::new(keys).font(mono.clone()).color(ACCENT));
                        ui.label(RichText::new(cmd.describe()).font(mono.clone()));
                        ui.end_row();
                    }
                });

                section(ui, "Mouse", &mono);
                pairs(ui, "help-mouse", MOUSE, &mono);
                section(ui, "Command line", &mono);
                pairs(ui, "help-ex", EX_COMMANDS, &mono);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Scroll with your scroll keys (j k, <C-d> <C-u>, gg G…); Esc, ? or q closes.",
                    )
                    .color(BAR_DIM),
                );
            });
            scroll.offset = out.state.clamped_offset().y;
            scroll.view_height = out.inner_rect.height();
            scroll.max = (out.content_size.y - out.inner_rect.height()).max(0.0);
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
