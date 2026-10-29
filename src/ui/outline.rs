//! The table of contents panel (`<Tab>`).

use eframe::egui::{
    self, Align, CursorIcon, FontId, Frame, Id, Label, Layout, Rect, RichText, ScrollArea, Sense,
    Vec2,
};

use super::{ACCENT, BAR_BACKGROUND, BAR_DIM, BAR_TEXT};
use crate::document::OutlineItem;
use crate::links::LinkTarget;

/// How the panel is drawn this frame.
pub struct Panel<'a> {
    /// Where the panel goes: the left strip of the page area.
    pub rect: Rect,
    pub items: &'a [OutlineItem],
    pub selected: usize,
    /// Scroll the list to keep `selected` in view.
    pub follow: bool,
    /// The panel has the keyboard (otherwise it is pinned and idle).
    pub focused: bool,
    pub pinned: bool,
    pub font_size: f32,
}

/// What the mouse did in the panel.
#[derive(Default)]
pub struct Outcome {
    /// Index of a clicked entry.
    pub clicked: Option<usize>,
    /// The right edge was dragged by this much.
    pub resized: f32,
}

/// Width of the strip at the right edge that resizes the panel.
const HANDLE: f32 = 6.0;

pub fn show(ctx: &egui::Context, panel: Panel) -> Outcome {
    let Panel {
        rect,
        items,
        selected,
        follow,
        focused,
        pinned,
        font_size,
    } = panel;
    let font = FontId::proportional(font_size);
    let mut out = Outcome::default();
    egui::Area::new(Id::unique("vimpdf-outline"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            Frame::NONE
                .fill(BAR_BACKGROUND)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_min_size(rect.size() - Vec2::splat(16.0));
                    ui.set_max_size(rect.size() - Vec2::splat(16.0));
                    let help = match (focused, pinned) {
                        (true, true) => "Contents  (j/k, Enter, p unpin, < >, Esc)",
                        (true, false) => "Contents  (j/k, Enter, p pin, < >, Esc)",
                        (false, _) => "Contents  (Tab to focus)",
                    };
                    ui.label(RichText::new(help).font(font.clone()).color(BAR_DIM));
                    ui.separator();
                    if items.is_empty() {
                        ui.label(RichText::new("This document has no outline.").color(BAR_DIM));
                        return;
                    }
                    // Page numbers get a fixed column so titles wrap before it.
                    let page_width = ui
                        .painter()
                        .layout_no_wrap("9999".into(), font.clone(), BAR_DIM)
                        .size()
                        .x;
                    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                        let full = ui.available_width();
                        for (i, item) in items.iter().enumerate() {
                            let page = match &item.target {
                                Some(LinkTarget::Internal { page, .. }) => (page + 1).to_string(),
                                _ => String::new(),
                            };
                            let color = if i == selected { ACCENT } else { BAR_TEXT };
                            // Deep levels stop indenting so titles keep room.
                            let indent = (item.depth as f32 * font_size).min(full * 0.3);
                            let title_width =
                                (full - indent - page_width - ui.spacing().item_spacing.x * 2.0)
                                    .max(40.0);
                            let response = ui
                                .horizontal_top(|ui| {
                                    ui.add_space(indent);
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(title_width, 0.0),
                                        Layout::top_down(Align::Min),
                                        |ui| {
                                            ui.set_width(title_width);
                                            ui.add(
                                                Label::new(
                                                    RichText::new(&item.title)
                                                        .font(font.clone())
                                                        .color(color),
                                                )
                                                .wrap()
                                                .selectable(false),
                                            );
                                        },
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                        ui.label(
                                            RichText::new(page).font(font.clone()).color(BAR_DIM),
                                        );
                                    });
                                })
                                .response
                                .interact(Sense::CLICK);
                            if i == selected {
                                let stroke = if focused { ACCENT } else { BAR_DIM };
                                ui.painter().rect_stroke(
                                    response.rect.expand(1.0),
                                    3.0,
                                    egui::Stroke::new(1.0, stroke),
                                    egui::StrokeKind::Outside,
                                );
                                if follow {
                                    response.scroll_to_me(Some(Align::Center));
                                }
                            }
                            if response.clicked() {
                                out.clicked = Some(i);
                            }
                        }
                    });
                });
            let handle = Rect::from_min_max(egui::pos2(rect.max.x - HANDLE, rect.min.y), rect.max);
            let response = ui
                .interact(handle, Id::unique("vimpdf-outline-resize"), Sense::DRAG)
                .on_hover_cursor(CursorIcon::ResizeHorizontal);
            if response.dragged() {
                out.resized = response.drag_delta().x;
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            if response.hovered() || response.dragged() {
                ui.painter().vline(
                    rect.max.x - 1.0,
                    rect.y_range(),
                    egui::Stroke::new(2.0, ACCENT),
                );
            }
        });
    out
}
