//! The annotations panel: every highlight, comment and redaction mark.

use eframe::egui::{
    self, Align, Color32, FontId, Frame, Id, Rect, RichText, ScrollArea, Sense, Vec2,
};

use super::{ACCENT, BAR_BACKGROUND, BAR_DIM, BAR_TEXT};
use crate::annotate::{AnnotInfo, AnnotKind};

/// One row: the annotation and the text it covers (if the page text is
/// loaded).
pub struct Row<'a> {
    pub annot: &'a AnnotInfo,
    pub snippet: String,
}

/// Draw the panel on the right of `area`. Returns the clicked row.
pub fn show(
    ctx: &egui::Context,
    area: Rect,
    rows: &[Row],
    selected: usize,
    follow: bool,
    font_size: f32,
) -> Option<usize> {
    let width = (area.width() * 0.4).clamp(260.0, 520.0);
    let rect = Rect::from_min_size(
        egui::pos2(area.max.x - width, area.min.y),
        Vec2::new(width, area.height()),
    );
    let font = FontId::proportional(font_size);
    let small = FontId::proportional((font_size - 2.0).max(9.0));
    let mut clicked = None;
    egui::Area::new(Id::unique("vimpdf-annotations"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            Frame::NONE.fill(BAR_BACKGROUND).inner_margin(8.0).show(ui, |ui| {
                ui.set_min_size(rect.size() - Vec2::splat(16.0));
                ui.set_max_size(rect.size() - Vec2::splat(16.0));
                ui.label(
                    RichText::new("Annotations  (j/k, Enter jump, e edit, c<letter> colour, m move, +/- text size, d delete, Esc)")
                        .font(small.clone())
                        .color(BAR_DIM),
                );
                ui.separator();
                if rows.is_empty() {
                    ui.label(
                        RichText::new("No annotations yet. Select text (v or the mouse), then a<letter> to highlight or c to comment; C adds a sticky note, T a text box.")
                            .color(BAR_DIM),
                    );
                    return;
                }
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    for (i, row) in rows.iter().enumerate() {
                        let a = row.annot;
                        let response = ui
                            .vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let [r, g, b] = a.color;
                                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(font_size * 0.8), Sense::hover());
                                    let fill = if a.kind == AnnotKind::Redact {
                                        Color32::BLACK
                                    } else {
                                        Color32::from_rgb(r, g, b)
                                    };
                                    ui.painter().rect_filled(dot, 2.0, fill);
                                    let color = if i == selected { ACCENT } else { BAR_TEXT };
                                    ui.label(
                                        RichText::new(format!("p. {}  {}", a.page + 1, a.kind.name()))
                                            .font(font.clone())
                                            .color(color),
                                    );
                                });
                                if !row.snippet.is_empty() {
                                    ui.label(
                                        RichText::new(format!("“{}”", row.snippet))
                                            .font(small.clone())
                                            .color(BAR_DIM),
                                    );
                                }
                                if !a.contents.is_empty() {
                                    ui.label(RichText::new(&a.contents).font(small.clone()).color(BAR_TEXT));
                                }
                            })
                            .response
                            .interact(Sense::CLICK);
                        if i == selected {
                            ui.painter().rect_stroke(
                                response.rect.expand(2.0),
                                3.0,
                                egui::Stroke::new(1.0, ACCENT),
                                egui::StrokeKind::Outside,
                            );
                            if follow {
                                response.scroll_to_me(Some(Align::Center));
                            }
                        }
                        if response.clicked() {
                            clicked = Some(i);
                        }
                        ui.add_space(4.0);
                    }
                });
            });
        });
    clicked
}
