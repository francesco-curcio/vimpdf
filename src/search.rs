//! Search: a background worker that scans pages from the current one and
//! wraps around, streaming matches per page; and the UI-side result state
//! for `n` / `N` and highlighting.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use crossbeam_channel::{Receiver, Sender, TryRecvError};

use crate::document;
use crate::text::{PageText, Span, TextRect};

pub struct SearchRequest {
    pub id: u64,
    pub query: String,
    pub case_sensitive: bool,
    pub start_page: usize,
    pub page_count: usize,
}

enum Request {
    Search(SearchRequest),
    /// Send a page's text to the UI (for selection and hit-testing).
    PageText(usize),
}

/// One match: its characters on the page and one rectangle per line.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchMatch {
    pub span: Span,
    pub rects: Vec<TextRect>,
}

pub enum SearchEvent {
    /// Matches on one page (only sent for pages with matches).
    Matches {
        id: u64,
        page: usize,
        matches: Vec<SearchMatch>,
    },
    /// Every page has been searched.
    Done {
        id: u64,
    },
    PageText {
        page: usize,
        text: Arc<PageText>,
    },
}

/// The text worker: searches, and serves page text to the UI.
pub struct Searcher {
    requests: Sender<Request>,
    events: Receiver<SearchEvent>,
}

impl Searcher {
    pub fn spawn(path: PathBuf, wake: impl Fn() + Send + 'static) -> Self {
        let (req_tx, req_rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("vimpdf-text".into())
            .spawn(move || Worker::new(path, ev_tx, wake).map(|w| w.run(req_rx)))
            .expect("failed to spawn text thread");
        Self {
            requests: req_tx,
            events: ev_rx,
        }
    }

    pub fn request(&self, req: SearchRequest) {
        self.send(Request::Search(req));
    }

    pub fn request_text(&self, page: usize) {
        self.send(Request::PageText(page));
    }

    fn send(&self, req: Request) {
        if self.requests.send(req).is_err() {
            tracing::error!("text worker is gone");
        }
    }

    pub fn try_recv(&self) -> Option<SearchEvent> {
        self.events.try_recv().ok()
    }
}

struct Worker<W> {
    path: PathBuf,
    doc: mupdf::Document,
    /// Text extraction is the slow part; keep every page's text once read.
    texts: HashMap<usize, Arc<PageText>>,
    events: Sender<SearchEvent>,
    wake: W,
}

impl<W: Fn()> Worker<W> {
    fn new(path: PathBuf, events: Sender<SearchEvent>, wake: W) -> Option<Self> {
        match document::open(&path) {
            Ok(doc) => Some(Self {
                path,
                doc,
                texts: HashMap::new(),
                events,
                wake,
            }),
            Err(e) => {
                tracing::error!("text worker cannot open document: {e:#}");
                None
            }
        }
    }

    fn text(&mut self, page: usize) -> Option<Arc<PageText>> {
        if let Some(t) = self.texts.get(&page) {
            return Some(Arc::clone(t));
        }
        match document::page_text(&self.doc, page) {
            Ok(t) => {
                let t = Arc::new(t);
                self.texts.insert(page, Arc::clone(&t));
                Some(t)
            }
            Err(e) => {
                tracing::warn!("cannot read text of page {}: {e:#}", page + 1);
                None
            }
        }
    }

    fn send(&self, event: SearchEvent) -> bool {
        let ok = self.events.send(event).is_ok();
        if ok {
            (self.wake)();
        }
        ok
    }

    fn send_text(&mut self, page: usize) -> bool {
        match self.text(page) {
            Some(text) => self.send(SearchEvent::PageText { page, text }),
            None => true,
        }
    }

    fn run(mut self, requests: Receiver<Request>) {
        let mut next = requests.recv().ok();
        while let Some(req) = next.take() {
            let search = match req {
                Request::PageText(page) => {
                    if !self.send_text(page) {
                        return;
                    }
                    next = requests.recv().ok();
                    continue;
                }
                Request::Search(s) => s,
            };
            match self.search(&search, &requests) {
                Ok(Some(newer)) => next = Some(newer),
                Ok(None) => next = requests.recv().ok(),
                Err(()) => return,
            }
        }
    }

    /// Run one search. Returns a newer search request that interrupted it,
    /// or `Err` if the UI is gone. Text requests arriving meanwhile are
    /// answered between pages.
    ///
    /// Reading a page's text is the slow part (MuPDF, ~10 ms a page), so
    /// when many pages haven't been read yet, helper threads (each with its
    /// own document) read them ahead, in search order, while this thread
    /// matches them as they arrive.
    fn search(
        &mut self,
        req: &SearchRequest,
        requests: &Receiver<Request>,
    ) -> Result<Option<Request>, ()> {
        let order: Vec<usize> = (req.start_page..req.page_count)
            .chain(0..req.start_page)
            .collect();
        let unread: Vec<usize> = order
            .iter()
            .copied()
            .filter(|p| !self.texts.contains_key(p))
            .collect();
        let helpers = helper_count(unread.len());
        if helpers == 0 {
            return self.scan(req, &order, requests, None);
        }
        let stop = AtomicBool::new(false);
        let next = AtomicUsize::new(0);
        let (tx, rx) = crossbeam_channel::unbounded();
        let path = self.path.clone();
        thread::scope(|scope| {
            for _ in 0..helpers {
                let (tx, unread, stop, next, path) = (tx.clone(), &unread, &stop, &next, &path);
                scope.spawn(move || {
                    let Ok(doc) = document::open(path) else {
                        return;
                    };
                    while !stop.load(Ordering::Relaxed) {
                        let Some(&page) = unread.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            return;
                        };
                        let text = document::page_text(&doc, page).ok().map(Arc::new);
                        if tx.send((page, text)).is_err() {
                            return;
                        }
                    }
                });
            }
            drop(tx);
            let result = self.scan(req, &order, requests, Some(&rx));
            // Interrupted or done: helpers stop after their current page.
            stop.store(true, Ordering::Relaxed);
            result
        })
    }

    /// Match pages in `order`, taking texts read ahead from `ahead`.
    fn scan(
        &mut self,
        req: &SearchRequest,
        order: &[usize],
        requests: &Receiver<Request>,
        ahead: Option<&Receiver<ReadAhead>>,
    ) -> Result<Option<Request>, ()> {
        let start = std::time::Instant::now();
        let mut total = 0;
        let mut failed = std::collections::HashSet::new();
        for &page in order {
            loop {
                match requests.try_recv() {
                    Ok(Request::PageText(p)) => {
                        if !self.send_text(p) {
                            return Err(());
                        }
                    }
                    Ok(newer @ Request::Search(_)) => return Ok(Some(newer)),
                    Err(TryRecvError::Disconnected) => return Err(()),
                    Err(TryRecvError::Empty) => break,
                }
            }
            // Wait for a helper to read it; if they're gone (or it failed
            // there), read it here.
            while let Some(rx) = ahead
                && !self.texts.contains_key(&page)
                && !failed.contains(&page)
            {
                match rx.recv() {
                    Ok((p, Some(text))) => {
                        self.texts.insert(p, text);
                    }
                    Ok((p, None)) => {
                        failed.insert(p);
                    }
                    Err(_) => break,
                }
            }
            let Some(text) = self.text(page) else {
                continue;
            };
            let matches: Vec<SearchMatch> = text
                .find(&req.query, req.case_sensitive)
                .into_iter()
                .map(|span| SearchMatch {
                    span,
                    rects: text.rects(span),
                })
                .collect();
            if !matches.is_empty() {
                total += matches.len();
                let id = req.id;
                if !self.send(SearchEvent::Matches { id, page, matches }) {
                    return Err(());
                }
            }
        }
        tracing::debug!(
            query = req.query,
            total,
            ms = start.elapsed().as_millis() as u64,
            "search done"
        );
        if self.send(SearchEvent::Done { id: req.id }) {
            Ok(None)
        } else {
            Err(())
        }
    }
}

/// A page's text read by a helper thread (`None`: it couldn't).
type ReadAhead = (usize, Option<Arc<PageText>>);

/// Helper threads to read `unread` pages with: none for a few pages (one
/// thread is quick enough, and opening documents isn't free).
fn helper_count(unread: usize) -> usize {
    if unread < 12 {
        return 0;
    }
    let cpus = thread::available_parallelism().map_or(2, |n| n.get());
    (cpus - 1).clamp(1, 6).min(unread / 4)
}

/// A match position: page and index within that page's matches.
pub type MatchId = (usize, usize);

/// Results of the current search, as seen by the UI.
#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub id: u64,
    pub running: bool,
    /// Hidden by Esc until the next n/N.
    pub highlight: bool,
    pub results: BTreeMap<usize, Vec<SearchMatch>>,
    pub current: Option<MatchId>,
    /// Where the search started: the first match at or after it is selected.
    origin: (usize, f32),
}

impl SearchState {
    pub fn start(&mut self, query: String, origin_page: usize, origin_y: f32) {
        self.id += 1;
        self.query = query;
        self.running = true;
        self.highlight = true;
        self.results.clear();
        self.current = None;
        self.origin = (origin_page, origin_y);
    }

    pub fn total(&self) -> usize {
        self.results.values().map(Vec::len).sum()
    }

    /// 1-based position of the current match in document order.
    pub fn position(&self) -> Option<usize> {
        let (page, idx) = self.current?;
        let before: usize = self.results.range(..page).map(|(_, m)| m.len()).sum();
        Some(before + idx + 1)
    }

    pub fn get(&self, (page, idx): MatchId) -> Option<&SearchMatch> {
        self.results.get(&page)?.get(idx)
    }

    /// Add one page's matches. Returns the match to jump to if this is the
    /// first one at or after the search origin.
    pub fn add(&mut self, page: usize, matches: Vec<SearchMatch>) -> Option<MatchId> {
        let (origin_page, origin_y) = self.origin;
        self.results.insert(page, matches);
        if self.current.is_some() || page < origin_page {
            return None;
        }
        let idx = if page == origin_page {
            self.results[&page]
                .iter()
                .position(|m| m.rects.first().is_some_and(|r| r.y0 >= origin_y))?
        } else {
            0
        };
        self.current = Some((page, idx));
        self.current
    }

    /// Search finished: if nothing came after the origin, wrap to the first
    /// match in the document.
    pub fn finish(&mut self) -> Option<MatchId> {
        self.running = false;
        if self.current.is_some() {
            return None;
        }
        let (&page, _) = self.results.iter().find(|(_, m)| !m.is_empty())?;
        self.current = Some((page, 0));
        self.current
    }

    /// Step `n` matches forward or backward from the current one, wrapping.
    pub fn step(&mut self, n: usize, forward: bool) -> Option<MatchId> {
        let all: Vec<MatchId> = self
            .results
            .iter()
            .flat_map(|(&p, m)| (0..m.len()).map(move |i| (p, i)))
            .collect();
        if all.is_empty() {
            return None;
        }
        let len = all.len();
        let pos = self
            .current
            .and_then(|c| all.iter().position(|m| *m == c))
            .unwrap_or(if forward { len - 1 } else { 0 });
        let n = n % len;
        let next = if forward {
            (pos + n) % len
        } else {
            (pos + len - n) % len
        };
        self.current = Some(all[next]);
        self.highlight = true;
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF of `n` pages; page i says "page i" and, on every third page,
    /// "needle".
    fn pdf(n: usize) -> Vec<u8> {
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {n} >>",
                (0..n)
                    .map(|i| format!("{} 0 R", 4 + 2 * i))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ];
        for i in 0..n {
            let extra = if i % 3 == 0 { " needle" } else { "" };
            let content = format!("BT /F1 12 Tf 72 700 Td (page {i}{extra}) Tj ET");
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents {} 0 R \
                 /Resources << /Font << /F1 3 0 R >> >> >>",
                5 + 2 * i
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            ));
        }
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    /// Run one search on a fresh worker; pages with matches, in the order
    /// they were reported.
    fn run(path: &std::path::Path, query: &str, start: usize, pages: usize) -> Vec<(usize, usize)> {
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (_req_tx, req_rx) = crossbeam_channel::unbounded();
        let mut worker = Worker::new(path.to_owned(), ev_tx, || {}).unwrap();
        let req = SearchRequest {
            id: 1,
            query: query.into(),
            case_sensitive: false,
            start_page: start,
            page_count: pages,
        };
        assert!(matches!(worker.search(&req, &req_rx), Ok(None)));
        ev_rx
            .try_iter()
            .filter_map(|e| match e {
                SearchEvent::Matches { page, matches, .. } => Some((page, matches.len())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn reading_ahead_on_helper_threads_finds_the_same_in_the_same_order() {
        assert_eq!(helper_count(5), 0, "a few pages: no helpers");
        assert!(helper_count(40) >= 1);
        let path = std::env::temp_dir().join(format!("vimpdf-search-{}.pdf", std::process::id()));
        std::fs::write(&path, pdf(40)).unwrap();
        // From page 10, wrapping around: 12, 15, … 39, then 0, 3, 6, 9.
        let expected: Vec<(usize, usize)> = (10..40)
            .chain(0..10)
            .filter(|p| p % 3 == 0)
            .map(|p| (p, 1))
            .collect();
        assert_eq!(run(&path, "needle", 10, 40), expected);
        assert_eq!(run(&path, "page 39", 0, 40), vec![(39, 1)]);
        assert_eq!(run(&path, "page 7", 0, 40), vec![(7, 1)]);
        std::fs::remove_file(&path).unwrap();
    }

    /// `VIMPDF_BENCH_PDF=file.pdf cargo test --release search_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn search_speed() {
        let path = std::path::PathBuf::from(std::env::var("VIMPDF_BENCH_PDF").unwrap());
        let pages = crate::document::DocInfo::load(&path).unwrap().page_count();
        let t = std::time::Instant::now();
        let doc = crate::document::open(&path).unwrap();
        for p in 0..pages {
            crate::document::page_text(&doc, p).unwrap();
        }
        println!("{pages} pages read on one thread: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let hits = run(&path, "the", 0, pages);
        println!(
            "searched with helpers, {} pages with hits: {:?}",
            hits.len(),
            t.elapsed()
        );
    }

    fn m(y: f32) -> SearchMatch {
        SearchMatch {
            span: (0, 0),
            rects: vec![TextRect {
                x0: 0.0,
                y0: y,
                x1: 10.0,
                y1: y + 10.0,
            }],
        }
    }

    #[test]
    fn first_match_after_origin_is_selected() {
        let mut s = SearchState::default();
        s.start("x".into(), 3, 200.0);
        assert_eq!(s.add(3, vec![m(50.0), m(300.0)]), Some((3, 1)));
        assert_eq!(s.add(5, vec![m(10.0)]), None, "already chose one");
        assert_eq!(s.position(), Some(2));
        assert_eq!(s.total(), 3);
    }

    #[test]
    fn wraps_when_nothing_follows_the_origin() {
        let mut s = SearchState::default();
        s.start("x".into(), 3, 200.0);
        assert_eq!(s.add(3, vec![m(50.0)]), None);
        assert_eq!(s.add(1, vec![m(10.0)]), None);
        assert_eq!(s.finish(), Some((1, 0)));
    }

    #[test]
    fn step_wraps_both_ways() {
        let mut s = SearchState::default();
        s.start("x".into(), 0, 0.0);
        s.add(0, vec![m(0.0), m(100.0)]);
        s.add(2, vec![m(0.0)]);
        s.finish();
        assert_eq!(s.current, Some((0, 0)));
        assert_eq!(s.step(1, true), Some((0, 1)));
        assert_eq!(s.step(1, true), Some((2, 0)));
        assert_eq!(s.step(1, true), Some((0, 0)));
        assert_eq!(s.step(1, false), Some((2, 0)));
        assert_eq!(
            s.step(5, true),
            Some((0, 1)),
            "5 steps from the last of 3 matches"
        );
    }

    #[test]
    fn no_results() {
        let mut s = SearchState::default();
        s.start("x".into(), 0, 0.0);
        assert_eq!(s.finish(), None);
        assert_eq!(s.step(1, true), None);
        assert_eq!(s.position(), None);
    }
}
