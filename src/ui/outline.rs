//! The table of contents panel (`<Tab>`).

use eframe::egui::{self, Align, FontId, Frame, Id, Rect, RichText, ScrollArea, Sense, Vec2};

use super::{ACCENT, BAR_BACKGROUND, BAR_DIM, BAR_TEXT};
use crate::document::OutlineItem;
use crate::links::LinkTarget;

/// Draw the panel over the left part of `area`. Returns the index of a
/// clicked entry. `follow` scrolls the list to keep `selected` in view.
pub fn show(
    ctx: &egui::Context,
    area: Rect,
    items: &[OutlineItem],
    selected: usize,
    follow: bool,
    font_size: f32,
) -> Option<usize> {
    let width = (area.width() * 0.4).clamp(220.0, 480.0);
    let rect = Rect::from_min_size(area.min, Vec2::new(width, area.height()));
    let font = FontId::proportional(font_size);
    let mut clicked = None;
    egui::Area::new(Id::unique("pdfv-outline"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            Frame::NONE
                .fill(BAR_BACKGROUND)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_min_size(rect.size() - Vec2::splat(16.0));
                    ui.set_max_size(rect.size() - Vec2::splat(16.0));
                    ui.label(
                        RichText::new("Contents  (j/k, Enter, Esc)")
                            .font(font.clone())
                            .color(BAR_DIM),
                    );
                    ui.separator();
                    if items.is_empty() {
                        ui.label(RichText::new("This document has no outline.").color(BAR_DIM));
                        return;
                    }
                    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                        for (i, item) in items.iter().enumerate() {
                            let page = match &item.target {
                                Some(LinkTarget::Internal { page, .. }) => (page + 1).to_string(),
                                _ => String::new(),
                            };
                            let color = if i == selected { ACCENT } else { BAR_TEXT };
                            let response = ui
                                .horizontal(|ui| {
                                    ui.add_space(item.depth as f32 * font_size);
                                    ui.label(
                                        RichText::new(&item.title).font(font.clone()).color(color),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(page)
                                                    .font(font.clone())
                                                    .color(BAR_DIM),
                                            );
                                        },
                                    );
                                })
                                .response
                                .interact(Sense::CLICK);
                            if i == selected {
                                ui.painter().rect_stroke(
                                    response.rect.expand(1.0),
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
                        }
                    });
                });
        });
    clicked
}
