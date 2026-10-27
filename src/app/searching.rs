//! Search in the app: starting searches, receiving results, n/N, highlights.

use super::*;

impl VimpdfApp {
    pub(super) fn start_search(&mut self, query: String, page: usize, origin_y: f32) {
        let query = if query.trim().is_empty() {
            self.view.search.query.clone()
        } else {
            query
        };
        if query.trim().is_empty() {
            return self.error("No previous search");
        }
        let search = &mut self.view.search;
        search.start(query.clone(), page, origin_y);
        self.view.searcher.request(SearchRequest {
            id: search.id,
            case_sensitive: text::is_case_sensitive(&query),
            query,
            start_page: page,
            page_count: self.view.page_count(),
        });
    }

    pub(super) fn search_step(&mut self, n: usize, forward: bool) {
        let search = &mut self.view.search;
        match search.step(n, forward) {
            Some(m) => self.show_match(m),
            None if search.running => self.info("Still searching…"),
            None if search.query.is_empty() => self.error("No previous search"),
            None => {
                let q = search.query.clone();
                self.error(format!("Pattern not found: {q}"));
            }
        }
    }

    /// Handle search results from the worker; jump to the first match.
    pub(super) fn receive_search(&mut self) {
        while let Some(event) = self.view.searcher.try_recv() {
            let search = &mut self.view.search;
            match event {
                SearchEvent::Matches { id, page, matches } if id == search.id => {
                    if let Some(m) = search.add(page, matches) {
                        self.show_match(m);
                    }
                }
                SearchEvent::Done { id } if id == search.id => {
                    if let Some(m) = search.finish() {
                        self.show_match(m);
                    } else if search.total() == 0 {
                        let q = search.query.clone();
                        self.error(format!("Pattern not found: {q}"));
                    }
                }
                SearchEvent::PageText { page, text } => self.view.add_text(page, text),
                _ => {} // results of an older search
            }
        }
    }

    /// Scroll so a search match is visible, a third of the way down.
    pub(super) fn show_match(&mut self, m: MatchId) {
        let Some(vp) = self.viewport.as_mut() else {
            return;
        };
        let Some(found) = self.view.search.get(m) else {
            return;
        };
        let caret = found.span.0;
        let Some(first) = found.rects.first() else {
            return;
        };
        let page = self.view.layout.page(m.0);
        let (x0, y0) = (page.x + first.x0, page.y + first.y0);
        let (x1, y1) = (page.x + first.x1, page.y + first.y1);
        if y0 < vp.y || y1 > vp.y + vp.doc_height() {
            let layout = &self.view.layout;
            let cur = vp.current_page(layout);
            self.jumps.push(Position {
                page: cur,
                offset: vp.y - layout.page(cur).y,
                x: vp.x,
            });
            vp.y = y0 - vp.doc_height() / 3.0;
        }
        if x0 < vp.x || x1 > vp.x + vp.doc_width() {
            vp.x = (x0 + x1) / 2.0 - vp.doc_width() / 2.0;
        }
        self.pending_scroll = Vec2::ZERO;
        self.caret_to_match(m.0, caret);
    }

    pub(super) fn draw_search_highlights(
        &self,
        painter: &egui::Painter,
        area: Rect,
        vp: &Viewport,
    ) {
        let search = &self.view.search;
        if !search.highlight || search.results.is_empty() {
            return;
        }
        let fill = color32(self.config.colors.search_highlight);
        let [r, g, b, _] = self.config.colors.hint_background.0;
        let current_fill = Color32::from_rgba_unmultiplied(r, g, b, 150);
        let layout = &self.view.layout;
        for page in layout.pages_in(vp.y, vp.y + vp.doc_height()) {
            let Some(matches) = search.results.get(&page) else {
                continue;
            };
            let p = layout.page(page);
            for (i, found) in matches.iter().enumerate() {
                let current = search.current == Some((page, i));
                for t in &found.rects {
                    let (ax, ay) = vp.to_view(p.x + t.x0, p.y + t.y0);
                    let (bx, by) = vp.to_view(p.x + t.x1, p.y + t.y1);
                    let rect = Rect::from_min_max(
                        area.min + Vec2::new(ax, ay),
                        area.min + Vec2::new(bx, by),
                    )
                    .expand(1.0);
                    painter.rect_filled(rect, 2.0, if current { current_fill } else { fill });
                }
            }
        }
    }

    pub(super) fn search_status(&self) -> String {
        let s = &self.view.search;
        if s.query.is_empty() {
            return String::new();
        }
        match (s.position(), s.running) {
            (Some(i), true) => format!("[{i}/{}+ searching…]", s.total()),
            (Some(i), false) => format!("[{i}/{}]", s.total()),
            (None, true) => "[searching…]".into(),
            (None, false) => String::new(),
        }
    }
}
