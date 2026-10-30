//! The tab bar at the top of the window.

use eframe::egui::{
    self, Align2, CursorIcon, FontId, PointerButton, Rect, Sense, Stroke, StrokeKind, Vec2,
};

use super::{ACCENT, BAR_BACKGROUND, BAR_DIM, BAR_TEXT};

pub struct TabLabel {
    pub name: String,
    pub unsaved: bool,
    pub active: bool,
}

/// What the mouse did, by tab index.
pub enum TabAction {
    Switch(usize),
    Close(usize),
    /// Move to a new window.
    Detach(usize),
    /// Move to the main window.
    Attach(usize),
}

const MIN_WIDTH: f32 = 90.0;
const MAX_WIDTH: f32 = 260.0;

pub fn height(font_size: f32) -> f32 {
    super::bar_height(font_size)
}

/// Draw the tabs in `rect`. Tabs share the width (up to a maximum each);
/// names that don't fit are shortened with "…". Each shows its number, for
/// `{n}gt`. A right click offers moving the tab to another window (to the
/// main one only from other windows).
pub fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    tabs: &[TabLabel],
    font_size: f32,
    main_window: bool,
) -> Option<TabAction> {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, BAR_BACKGROUND);
    let font = FontId::proportional(font_size * 0.95);
    let width =
        (rect.width() / tabs.len().max(1) as f32).clamp(MIN_WIDTH.min(rect.width()), MAX_WIDTH);
    let close_size = font_size;
    let mut action = None;
    for (i, tab) in tabs.iter().enumerate() {
        let x = rect.min.x + i as f32 * width;
        if x >= rect.max.x {
            break;
        }
        let tab_rect =
            Rect::from_min_size(egui::pos2(x, rect.min.y), Vec2::new(width, rect.height()))
                .intersect(rect);
        let id = egui::Id::unique(("vimpdf-tab", i));
        let response = ui.interact(tab_rect, id, Sense::click());
        let close_rect = Rect::from_center_size(
            egui::pos2(tab_rect.max.x - close_size, tab_rect.center().y),
            Vec2::splat(close_size),
        );
        let close = ui.interact(close_rect, id.with("close"), Sense::click());

        let fill = if tab.active {
            egui::Color32::from_gray(48)
        } else if response.hovered() {
            egui::Color32::from_gray(36)
        } else {
            BAR_BACKGROUND
        };
        painter.rect_filled(tab_rect.shrink2(Vec2::new(1.0, 0.0)), 0.0, fill);
        if tab.active {
            painter.line_segment(
                [
                    egui::pos2(tab_rect.min.x + 1.0, tab_rect.max.y - 1.0),
                    egui::pos2(tab_rect.max.x - 1.0, tab_rect.max.y - 1.0),
                ],
                Stroke::new(2.0, ACCENT),
            );
        }

        // "2 paper.pdf [+]", shortened to fit before the close button.
        let color = if tab.active { BAR_TEXT } else { BAR_DIM };
        let suffix = if tab.unsaved { " [+]" } else { "" };
        let room = (close_rect.min.x - tab_rect.min.x - 12.0).max(0.0);
        let text = fit(ui, &format!("{} {}", i + 1, tab.name), suffix, &font, room);
        painter.text(
            egui::pos2(tab_rect.min.x + 8.0, tab_rect.center().y),
            Align2::LEFT_CENTER,
            text,
            font.clone(),
            color,
        );

        if tab.active || response.hovered() || close.hovered() {
            let c = if close.hovered() { BAR_TEXT } else { BAR_DIM };
            let r = close_rect.shrink(close_size * 0.3);
            painter.line_segment([r.left_top(), r.right_bottom()], Stroke::new(1.5, c));
            painter.line_segment([r.right_top(), r.left_bottom()], Stroke::new(1.5, c));
            if close.hovered() {
                painter.rect_stroke(
                    close_rect,
                    3.0,
                    Stroke::new(1.0, BAR_DIM),
                    StrokeKind::Inside,
                );
            }
        }

        response.context_menu(|ui| {
            let mut entry = |label: &str, enabled: bool, chosen: TabAction| {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    action = Some(chosen);
                    ui.close();
                }
            };
            entry("Move to new window", tabs.len() > 1, TabAction::Detach(i));
            if !main_window {
                entry("Move to main window", true, TabAction::Attach(i));
            }
            entry("Close tab", true, TabAction::Close(i));
        });

        if close.clicked() || response.clicked_by(PointerButton::Middle) {
            action = Some(TabAction::Close(i));
        } else if response.clicked() && !tab.active {
            action = Some(TabAction::Switch(i));
        }
        if response.hovered() || close.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Default);
        }
    }
    action
}

/// `text` + `suffix`, with `text` cut and "…" added until it fits `room`.
fn fit(ui: &egui::Ui, text: &str, suffix: &str, font: &FontId, room: f32) -> String {
    let width = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), BAR_TEXT)
            .size()
            .x
    };
    let full = format!("{text}{suffix}");
    if width(&full) <= room {
        return full;
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let s = format!("{}…{suffix}", chars.iter().collect::<String>());
        if width(&s) <= room {
            return s;
        }
    }
    suffix.trim().to_owned()
}
