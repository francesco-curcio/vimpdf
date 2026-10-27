//! The File / Edit / View menu bar: drawn by hand so it can be driven fully
//! from the keyboard (vim keys, Enter, Esc, underlined letters) as well as
//! with the mouse.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Id, Rect, Sense, Stroke, Vec2,
    text::{LayoutJob, TextFormat},
};

use super::{ACCENT, BAR_BACKGROUND, BAR_DIM, BAR_TEXT};
use crate::input::command::Command;

/// One entry of a menu.
pub enum Item {
    Action {
        /// Label with `&` before the underlined letter ("&Open…").
        label: String,
        /// The key shown on the right (current binding or `:` command).
        key: String,
        command: Command,
        enabled: bool,
        /// `Some(on)` shows a check mark.
        checked: Option<bool>,
    },
    Submenu {
        label: String,
        items: Vec<Item>,
    },
    Separator,
}

impl Item {
    pub fn action(label: &str, key: impl Into<String>, command: Command) -> Self {
        Item::Action {
            label: label.into(),
            key: key.into(),
            command,
            enabled: true,
            checked: None,
        }
    }

    pub fn toggle(label: &str, key: impl Into<String>, command: Command, on: bool) -> Self {
        Item::Action {
            label: label.into(),
            key: key.into(),
            command,
            enabled: true,
            checked: Some(on),
        }
    }

    pub fn enabled(mut self, on: bool) -> Self {
        if let Item::Action { enabled, .. } = &mut self {
            *enabled = on;
        }
        self
    }

    fn label(&self) -> Option<&str> {
        match self {
            Item::Action { label, .. } | Item::Submenu { label, .. } => Some(label),
            Item::Separator => None,
        }
    }

    fn selectable(&self) -> bool {
        !matches!(self, Item::Separator)
    }
}

pub struct Menu {
    pub label: String,
    pub items: Vec<Item>,
}

/// The underlined letter of a label (lowercase), and the label without `&`.
pub fn mnemonic(label: &str) -> (Option<char>, String) {
    let mut letter = None;
    let mut text = String::new();
    let mut chars = label.chars();
    while let Some(c) = chars.next() {
        if c == '&'
            && let Some(next) = chars.next()
        {
            letter.get_or_insert(next.to_ascii_lowercase());
            text.push(next);
        } else {
            text.push(c);
        }
    }
    (letter, text)
}

/// Which menu and entries are open.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MenuState {
    /// Open menu, or `None` when the menus are closed.
    pub menu: Option<usize>,
    /// Highlighted entry of the open menu.
    pub item: usize,
    /// Open submenu (index of its entry) and its highlighted entry.
    pub sub: Option<(usize, usize)>,
}

/// What a key or click did.
#[derive(Debug, PartialEq)]
pub enum MenuAction {
    None,
    Close,
    Run(Command),
}

fn next_selectable(items: &[Item], from: usize, forward: bool) -> usize {
    let n = items.len();
    if n == 0 {
        return 0;
    }
    let mut i = from;
    for _ in 0..n {
        i = if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        };
        if items[i].selectable() {
            return i;
        }
    }
    from
}

fn first_selectable(items: &[Item]) -> usize {
    items.iter().position(Item::selectable).unwrap_or(0)
}

impl MenuState {
    pub fn is_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn open(&mut self, menus: &[Menu], menu: usize) {
        self.menu = Some(menu.min(menus.len().saturating_sub(1)));
        self.item = first_selectable(&menus[self.menu.unwrap_or(0)].items);
        self.sub = None;
    }

    /// Activate an entry: run it, or open its submenu.
    fn activate(&mut self, items: &[Item], i: usize, in_sub: bool) -> MenuAction {
        match &items[i] {
            Item::Action {
                command, enabled, ..
            } if *enabled => MenuAction::Run(command.clone()),
            Item::Submenu { items: sub, .. } if !in_sub => {
                self.sub = Some((i, first_selectable(sub)));
                MenuAction::None
            }
            _ => MenuAction::None,
        }
    }

    /// Handle a key token (vim notation) while a menu is open.
    pub fn key(&mut self, menus: &[Menu], tok: &str) -> MenuAction {
        let Some(m) = self.menu else {
            return MenuAction::None;
        };
        let items = &menus[m].items;
        let in_sub = self.sub;
        let sub_items = in_sub.and_then(|(i, _)| match &items[i] {
            Item::Submenu { items, .. } => Some(items.as_slice()),
            _ => None,
        });
        match (tok, sub_items) {
            ("<Esc>", Some(_)) => self.sub = None,
            ("<Esc>" | "<F10>", None) => return MenuAction::Close,
            ("j" | "<Down>", Some(s)) => {
                let (i, j) = in_sub.expect("submenu open");
                self.sub = Some((i, next_selectable(s, j, true)));
            }
            ("k" | "<Up>", Some(s)) => {
                let (i, j) = in_sub.expect("submenu open");
                self.sub = Some((i, next_selectable(s, j, false)));
            }
            ("j" | "<Down>", None) => self.item = next_selectable(items, self.item, true),
            ("k" | "<Up>", None) => self.item = next_selectable(items, self.item, false),
            ("h" | "<Left>", Some(_)) => self.sub = None,
            ("h" | "<Left>", None) => self.open(menus, (m + menus.len() - 1) % menus.len()),
            ("l" | "<Right>", None) if matches!(items[self.item], Item::Submenu { .. }) => {
                return self.activate(items, self.item, false);
            }
            ("l" | "<Right>", _) => self.open(menus, (m + 1) % menus.len()),
            ("<CR>" | "<Space>", Some(s)) => {
                let (_, j) = in_sub.expect("submenu open");
                return self.activate(s, j, true);
            }
            ("<CR>" | "<Space>", None) => return self.activate(items, self.item, false),
            (t, list) if t.chars().count() == 1 => {
                // An underlined letter runs its entry directly.
                let c = t.chars().next().expect("one char").to_ascii_lowercase();
                let (list, is_sub) = match list {
                    Some(s) => (s, true),
                    None => (items.as_slice(), false),
                };
                let found = list
                    .iter()
                    .position(|it| it.label().is_some_and(|l| mnemonic(l).0 == Some(c)));
                if let Some(i) = found {
                    if is_sub {
                        let (p, _) = in_sub.expect("submenu open");
                        self.sub = Some((p, i));
                    } else {
                        self.item = i;
                    }
                    return self.activate(list, i, is_sub);
                }
            }
            _ => {}
        }
        MenuAction::None
    }
}

fn underlined(label: &str, font: &FontId, color: Color32) -> LayoutJob {
    let (letter, _) = mnemonic(label);
    let mut job = LayoutJob::default();
    let mut underline_next = false;
    let mut done = false;
    for c in label.chars() {
        if c == '&' && !done {
            underline_next = true;
            continue;
        }
        let mut fmt = TextFormat::simple(font.clone(), color);
        if underline_next && letter.is_some() {
            fmt.underline = Stroke::new(1.0, color);
            underline_next = false;
            done = true;
        }
        job.append(&c.to_string(), 0.0, fmt);
    }
    job
}

/// Draw the bar (and the open menu). Returns the action a click triggered.
pub fn show(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    rect: Rect,
    menus: &[Menu],
    state: &mut MenuState,
    font_size: f32,
) -> MenuAction {
    let font = FontId::proportional(font_size);
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, BAR_BACKGROUND);
    let mut action = MenuAction::None;
    let mut x = rect.min.x + 8.0;
    let mut title_rects = Vec::new();
    for (i, menu) in menus.iter().enumerate() {
        let galley = painter.layout_job(underlined(&menu.label, &font, BAR_TEXT));
        let r = Rect::from_min_size(
            egui::pos2(x, rect.min.y),
            Vec2::new(galley.size().x + 16.0, rect.height()),
        );
        if state.menu == Some(i) {
            painter.rect_filled(r, 3.0, ACCENT.gamma_multiply(0.35));
        }
        painter.galley(
            egui::pos2(r.min.x + 8.0, r.center().y - galley.size().y / 2.0),
            galley,
            BAR_TEXT,
        );
        let resp = ui
            .interact(r, Id::unique(("pdfv-menu-title", i)), Sense::CLICK)
            .on_hover_cursor(CursorIcon::PointingHand);
        if resp.clicked() {
            if state.menu == Some(i) {
                action = MenuAction::Close;
            } else {
                state.open(menus, i);
            }
        } else if resp.hovered() && state.is_open() && state.menu != Some(i) {
            state.open(menus, i);
        }
        title_rects.push(r);
        x = r.max.x + 2.0;
    }

    let Some(m) = state.menu else { return action };
    let anchor = title_rects[m].left_bottom();
    let mut hovered_anything = false;
    let popup = |ctx: &egui::Context,
                 id: &str,
                 pos: egui::Pos2,
                 items: &[Item],
                 selected: usize,
                 is_sub: bool,
                 state: &mut MenuState,
                 hovered_anything: &mut bool|
     -> (MenuAction, Vec<Rect>) {
        let mut action = MenuAction::None;
        let mut rows = Vec::new();
        let small = FontId::proportional((font_size - 1.0).max(10.0));
        let resp = egui::Area::new(Id::unique(id))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(260.0);
                    for (i, item) in items.iter().enumerate() {
                        if matches!(item, Item::Separator) {
                            ui.separator();
                            rows.push(Rect::NOTHING);
                            continue;
                        }
                        let (label, key, enabled, checked, sub) = match item {
                            Item::Action {
                                label,
                                key,
                                enabled,
                                checked,
                                ..
                            } => (label.as_str(), key.as_str(), *enabled, *checked, false),
                            Item::Submenu { label, .. } => (label.as_str(), "▸", true, None, true),
                            Item::Separator => unreachable!(),
                        };
                        let height = font_size * 1.6;
                        let (row, resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width().max(260.0), height),
                            Sense::CLICK,
                        );
                        rows.push(row);
                        let p = ui.painter();
                        if i == selected {
                            p.rect_filled(row, 3.0, ACCENT.gamma_multiply(0.35));
                        }
                        let color = if enabled { BAR_TEXT } else { BAR_DIM };
                        if let Some(on) = checked {
                            p.text(
                                row.left_center() + Vec2::new(6.0, 0.0),
                                Align2::LEFT_CENTER,
                                if on { "✔" } else { "" },
                                small.clone(),
                                color,
                            );
                        }
                        let galley = p.layout_job(underlined(label, &font, color));
                        p.galley(
                            row.left_center() + Vec2::new(24.0, -galley.size().y / 2.0),
                            galley,
                            color,
                        );
                        p.text(
                            row.right_center() - Vec2::new(8.0, 0.0),
                            Align2::RIGHT_CENTER,
                            key,
                            small.clone(),
                            BAR_DIM,
                        );
                        if resp.hovered() {
                            *hovered_anything = true;
                            if is_sub {
                                if let Some((p, _)) = state.sub {
                                    state.sub = Some((p, i));
                                }
                            } else if state.item != i {
                                state.item = i;
                                state.sub = None;
                                if sub {
                                    state.activate(items, i, false);
                                }
                            }
                        }
                        if resp.clicked() {
                            action = state.activate(items, i, is_sub);
                        }
                    }
                });
            })
            .response;
        if resp.hovered() {
            *hovered_anything = true;
        }
        (action, rows)
    };

    let items = &menus[m].items;
    let (a, rows) = popup(
        ctx,
        "pdfv-menu-popup",
        anchor,
        items,
        state.item,
        false,
        state,
        &mut hovered_anything,
    );
    if a != MenuAction::None {
        action = a;
    }
    if let Some((parent, sel)) = state.sub
        && let Some(Item::Submenu { items: sub, .. }) = items.get(parent)
        && let Some(row) = rows.get(parent)
    {
        let (a, _) = popup(
            ctx,
            "pdfv-menu-sub",
            row.right_top() + Vec2::new(8.0, -4.0),
            sub,
            sel,
            true,
            state,
            &mut hovered_anything,
        );
        if a != MenuAction::None {
            action = a;
        }
    }

    let clicked_elsewhere = ctx.input(|i| i.pointer.any_pressed())
        && !hovered_anything
        && !title_rects.iter().any(|r| {
            ctx.input(|i| i.pointer.interact_pos())
                .is_some_and(|p| r.contains(p))
        });
    if clicked_elsewhere {
        action = MenuAction::Close;
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menus() -> Vec<Menu> {
        vec![
            Menu {
                label: "&File".into(),
                items: vec![
                    Item::action("&Open…", ":open", Command::EnterCommandLine("open ".into())),
                    Item::Separator,
                    Item::action("&Quit", "q", Command::Quit),
                ],
            },
            Menu {
                label: "&View".into(),
                items: vec![
                    Item::action("Zoom &in", "+", Command::ZoomIn(1)),
                    Item::Submenu {
                        label: "&Colour".into(),
                        items: vec![
                            Item::action("&Yellow", "ay", Command::Highlight(Some('y'))),
                            Item::action("&Green", "ag", Command::Highlight(Some('g'))),
                        ],
                    },
                    Item::action("Disabled", "", Command::Undo).enabled(false),
                ],
            },
        ]
    }

    #[test]
    fn mnemonics() {
        assert_eq!(mnemonic("&Open…"), (Some('o'), "Open…".into()));
        assert_eq!(mnemonic("Zoom &in"), (Some('i'), "Zoom in".into()));
        assert_eq!(mnemonic("Plain"), (None, "Plain".into()));
    }

    #[test]
    fn navigation_skips_separators_and_wraps() {
        let m = menus();
        let mut s = MenuState::default();
        s.open(&m, 0);
        assert_eq!(s.item, 0);
        assert_eq!(s.key(&m, "j"), MenuAction::None);
        assert_eq!(s.item, 2, "separator skipped");
        s.key(&m, "j");
        assert_eq!(s.item, 0, "wraps");
        s.key(&m, "l");
        assert_eq!(s.menu, Some(1));
        s.key(&m, "h");
        assert_eq!(s.menu, Some(0));
        assert_eq!(
            s.key(&m, "<CR>"),
            MenuAction::Run(Command::EnterCommandLine("open ".into()))
        );
        assert_eq!(s.key(&m, "<Esc>"), MenuAction::Close);
    }

    #[test]
    fn submenus_and_letters() {
        let m = menus();
        let mut s = MenuState::default();
        s.open(&m, 1);
        s.key(&m, "j");
        assert_eq!(s.key(&m, "l"), MenuAction::None, "l opens the submenu");
        assert_eq!(s.sub, Some((1, 0)));
        s.key(&m, "j");
        assert_eq!(
            s.key(&m, "<CR>"),
            MenuAction::Run(Command::Highlight(Some('g')))
        );
        s.key(&m, "h");
        assert_eq!(s.sub, None, "h leaves the submenu");
        assert_eq!(
            s.key(&m, "i"),
            MenuAction::Run(Command::ZoomIn(1)),
            "underlined letter"
        );
        assert_eq!(s.key(&m, "c"), MenuAction::None);
        assert_eq!(
            s.key(&m, "y"),
            MenuAction::Run(Command::Highlight(Some('y')))
        );
        s.sub = None;
        s.item = 2;
        assert_eq!(
            s.key(&m, "<CR>"),
            MenuAction::None,
            "disabled entries don't run"
        );
    }
}
