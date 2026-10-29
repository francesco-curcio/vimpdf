//! The start page: shown instead of a document when vimpdf starts without
//! one (or in a `:tabnew` tab). It lists the recently read files, like
//! Okular's welcome screen.

use eframe::egui::{Align, Frame, Label, Layout, RichText, ScrollArea, Sense};

use super::*;
use crate::ui::{ACCENT, BAR_BACKGROUND, BAR_DIM, BAR_TEXT};

const RECENT: usize = 20;

pub(super) struct StartScreen {
    recent: Vec<Recent>,
    selected: usize,
    /// Scroll the list to the selection on the next frame.
    follow: bool,
}

struct Recent {
    path: PathBuf,
    /// Page last read (1-based).
    page: Option<usize>,
}

impl StartScreen {
    fn load() -> Self {
        let recent = crate::state::state_dir()
            .map(|dir| {
                crate::state::recent(&dir, RECENT + 1)
                    .into_iter()
                    .filter(|p| !crate::welcome::is_welcome(p))
                    .take(RECENT)
                    .map(|path| Recent {
                        page: crate::state::load(&dir, &path).map(|s| s.position.page + 1),
                        path,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            recent,
            selected: 0,
            follow: false,
        }
    }
}

/// `/Users/me/papers` → `~/papers`.
fn short_dir(path: &std::path::Path) -> String {
    let dir = path.parent().unwrap_or(path);
    if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_owned())
        && let Ok(rest) = dir.strip_prefix(&home)
    {
        return format!("~/{}", rest.display());
    }
    dir.display().to_string()
}

impl VimpdfApp {
    pub(super) fn on_start_page(&self) -> bool {
        crate::welcome::is_welcome(&self.view.path)
    }

    /// Load the recent files when the start page shows, drop them when it
    /// doesn't (so the list is fresh each time).
    pub(super) fn update_start_screen(&mut self) {
        match (self.on_start_page(), self.start.is_some()) {
            (true, false) => self.start = Some(StartScreen::load()),
            (false, true) => self.start = None,
            _ => {}
        }
    }

    /// A key on the start page; `None` if it's not one of the page's own
    /// keys (then it goes through the normal keymap: `:`, `?`, `q`, `gt`…).
    pub(super) fn start_key(&mut self, tok: &str) -> Option<Vec<Command>> {
        let start = self.start.as_mut()?;
        let last = start.recent.len().saturating_sub(1);
        match tok {
            "j" | "<Down>" | "<C-n>" => start.selected = (start.selected + 1).min(last),
            "k" | "<Up>" | "<C-p>" => start.selected = start.selected.saturating_sub(1),
            "g" | "<Home>" => start.selected = 0,
            "G" | "<End>" => start.selected = last,
            "<CR>" | "o" | "l" => {
                let path = start.recent.get(start.selected)?.path.clone();
                return Some(vec![Command::Open(path)]);
            }
            _ => {
                // 1-9 open that entry directly.
                let digit = tok.parse::<usize>().ok().filter(|d| (1..=9).contains(d))?;
                let path = start.recent.get(digit - 1)?.path.clone();
                return Some(vec![Command::Open(path)]);
            }
        }
        start.follow = true;
        Some(Vec::new())
    }

    /// Draw the start page over `area`. Returns a file to open if one was
    /// clicked.
    pub(super) fn show_start_screen(&mut self, ui: &mut egui::Ui, area: Rect) -> Option<Command> {
        let font_size = self.config.font_size;
        let start = self.start.as_mut()?;
        ui.painter().rect_filled(area, 0.0, BACKGROUND);
        let width = area.width().min(760.0) - 32.0;
        let inner = Rect::from_min_size(
            egui::pos2(area.center().x - width / 2.0, area.min.y + 32.0),
            Vec2::new(width, (area.height() - 64.0).max(0.0)),
        );
        let mut clicked = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.label(
                RichText::new("vimpdf")
                    .size(font_size * 2.0)
                    .color(BAR_TEXT),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Open a file with :open <path>, drop one on the window, or pick a recent \
                     one: j/k and Enter, or its number. ? shows the keys, q quits.",
                )
                .size(font_size)
                .color(BAR_DIM),
            );
            ui.add_space(16.0);
            ui.label(
                RichText::new("Recent files")
                    .size(font_size * 1.2)
                    .color(BAR_TEXT),
            );
            ui.add_space(4.0);
            if start.recent.is_empty() {
                ui.label(
                    RichText::new("Nothing read yet.")
                        .size(font_size)
                        .color(BAR_DIM),
                );
                return;
            }
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for (i, entry) in start.recent.iter().enumerate() {
                    let selected = i == start.selected;
                    let name = entry
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let response = Frame::NONE
                        .fill(if selected {
                            egui::Color32::from_gray(52)
                        } else {
                            BAR_BACKGROUND
                        })
                        .inner_margin(8.0)
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let number = if i < 9 {
                                    format!("{} ", i + 1)
                                } else {
                                    "  ".into()
                                };
                                ui.label(
                                    RichText::new(number)
                                        .monospace()
                                        .size(font_size)
                                        .color(ACCENT),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if let Some(page) = entry.page {
                                        ui.label(
                                            RichText::new(format!("page {page}"))
                                                .size(font_size * 0.85)
                                                .color(BAR_DIM),
                                        );
                                    }
                                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                        ui.add(
                                            Label::new(
                                                RichText::new(name).size(font_size * 1.05).color(
                                                    if selected { ACCENT } else { BAR_TEXT },
                                                ),
                                            )
                                            .truncate()
                                            .selectable(false),
                                        );
                                        ui.add(
                                            Label::new(
                                                RichText::new(short_dir(&entry.path))
                                                    .size(font_size * 0.85)
                                                    .color(BAR_DIM),
                                            )
                                            .truncate()
                                            .selectable(false),
                                        );
                                    });
                                });
                            });
                        })
                        .response
                        .interact(Sense::CLICK);
                    if selected && start.follow {
                        response.scroll_to_me(Some(Align::Center));
                    }
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if response.clicked() {
                        clicked = Some(entry.path.clone());
                    }
                    ui.add_space(4.0);
                }
            });
        });
        start.follow = false;
        clicked.map(Command::Open)
    }
}
