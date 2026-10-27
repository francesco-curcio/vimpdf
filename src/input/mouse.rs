//! Mouse and trackpad input → `Command`s, plus hit-testing.

use eframe::egui::{self, CursorIcon, Pos2, Rect};

use super::command::Command;
use crate::layout::Layout;
use crate::links::PageLink;
use crate::viewport::Viewport;

/// What the mouse did this frame.
pub struct MouseOutput {
    pub commands: Vec<Command>,
    pub cursor: CursorIcon,
}

pub fn handle(
    ctx: &egui::Context,
    view_rect: Rect,
    viewport: &Viewport,
    layout: &Layout,
    links: &[Vec<PageLink>],
) -> MouseOutput {
    let mut commands = Vec::new();
    let (scroll, zoom, hover, clicked, click_pos) = ctx.input(|i| {
        (
            i.smooth_scroll_delta,
            i.zoom_delta(),
            i.pointer.hover_pos(),
            i.pointer.primary_clicked(),
            i.pointer.interact_pos(),
        )
    });

    // egui reports how the content should move; scrolling moves the view the
    // opposite way.
    if scroll != egui::Vec2::ZERO {
        commands.push(Command::ScrollBy {
            dx: -scroll.x,
            dy: -scroll.y,
        });
    }

    // Pinch, or Cmd/Ctrl + wheel (egui turns those into zoom, not scroll).
    if zoom != 1.0 {
        let anchor = hover
            .filter(|p| view_rect.contains(*p))
            .unwrap_or(view_rect.center())
            - view_rect.min;
        commands.push(Command::ZoomBy {
            factor: zoom,
            x: anchor.x,
            y: anchor.y,
        });
    }

    let link_at = |pos: Option<Pos2>| {
        pos.filter(|p| view_rect.contains(*p))
            .and_then(|p| link_at(p - view_rect.min.to_vec2(), viewport, layout, links))
    };

    if clicked && let Some(link) = link_at(click_pos) {
        commands.push(Command::FollowLink(link.target.clone()));
    }

    let cursor = if link_at(hover).is_some() {
        CursorIcon::PointingHand
    } else {
        CursorIcon::Default
    };
    MouseOutput { commands, cursor }
}

/// The link under a point given in view coordinates.
pub fn link_at<'a>(
    view_pos: Pos2,
    viewport: &Viewport,
    layout: &Layout,
    links: &'a [Vec<PageLink>],
) -> Option<&'a PageLink> {
    let (x, y) = viewport.to_doc(view_pos.x, view_pos.y);
    let page = layout.page_containing(x, y)?;
    let rect = layout.page(page);
    let (px, py) = (x - rect.x, y - rect.y);
    links.get(page)?.iter().find(|l| l.contains(px, py))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::PageSize;
    use crate::links::LinkTarget;

    #[test]
    fn hit_tests_links_in_page_coordinates() {
        let layout = Layout::new(&[PageSize::new(500.0, 700.0); 2], 10.0);
        let mut vp = Viewport::new(520.0, 400.0);
        vp.fit_width(&layout, 10.0);
        vp.clamp(&layout);
        vp.goto_page(&layout, 1);

        let link = PageLink {
            x0: 100.0,
            y0: 50.0,
            x1: 200.0,
            y1: 70.0,
            target: LinkTarget::External("https://arxiv.org".into()),
        };
        let links = vec![vec![], vec![link.clone()]];

        // View x = 10 is the page's left edge (centred with a 10pt margin).
        assert_eq!(
            link_at(Pos2::new(10.0 + 150.0, 60.0), &vp, &layout, &links),
            Some(&link)
        );
        assert_eq!(
            link_at(Pos2::new(10.0 + 50.0, 60.0), &vp, &layout, &links),
            None
        );
    }
}
