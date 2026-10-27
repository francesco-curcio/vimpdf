//! Annotations: highlights, comments and redaction marks, stored as standard
//! PDF annotations.
//!
//! An editor thread owns a writable MuPDF `PdfDocument`. Every change is
//! applied there and then written to a fresh *working copy* file that the
//! render workers switch to, so the change shows without touching the
//! original. `:w` saves into the original (incrementally: the original bytes
//! are kept and the changes appended).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread;

use anyhow::{Context, Result, anyhow, bail};
use crossbeam_channel::{Receiver, Sender};
use mupdf::color::AnnotationColor;
use mupdf::pdf::{
    AnnotationQuadPoints, PdfAnnotation, PdfAnnotationType, PdfDocument, PdfPage, PdfWriteOptions,
};
use mupdf::{Point, Quad, Rect};

use crate::text::TextRect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotKind {
    Highlight,
    /// A sticky note (`Text` annotation).
    Note,
    /// A redaction mark, applied by `:redact`.
    Redact,
    /// Any other kind (ink, stamps…): listed, not editable.
    Other,
}

impl AnnotKind {
    pub fn name(self) -> &'static str {
        match self {
            AnnotKind::Highlight => "highlight",
            AnnotKind::Note => "note",
            AnnotKind::Redact => "redaction",
            AnnotKind::Other => "annotation",
        }
    }
}

/// An annotation as plain data, in page-relative coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotInfo {
    pub page: usize,
    /// The annotation's PDF object number: its identity.
    pub xref: i32,
    pub kind: AnnotKind,
    pub color: [u8; 3],
    pub rects: Vec<TextRect>,
    pub contents: String,
}

/// What a new annotation looks like.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAnnot {
    pub kind: AnnotKind,
    /// One entry per page: the rectangles to cover.
    pub parts: Vec<(usize, Vec<TextRect>)>,
    pub color: [u8; 3],
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditRequest {
    Create(NewAnnot),
    SetContents {
        page: usize,
        xref: i32,
        contents: String,
    },
    SetColor {
        page: usize,
        xref: i32,
        color: [u8; 3],
    },
    Delete {
        page: usize,
        xref: i32,
    },
    Undo,
    /// Save into the original file (`None`) or a copy.
    Save {
        path: Option<PathBuf>,
    },
    /// Apply every redaction mark to a copy written to `out`.
    ApplyRedactions {
        out: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub enum EditEvent {
    /// All annotations of the document, sent once after opening.
    Loaded(Vec<AnnotInfo>),
    /// After a change: all annotations, the pages to re-render and the
    /// working copy that now holds the document.
    Changed {
        annots: Vec<AnnotInfo>,
        pages: Vec<usize>,
        working_copy: PathBuf,
        unsaved: bool,
        /// Unsaved changes other than redaction marks.
        unsaved_other: bool,
    },
    /// Redactions applied and written to `path`; `leftover` lists marks under
    /// which text was still found when the written file was re-read (empty
    /// when the check passed).
    Redacted {
        path: PathBuf,
        marks: usize,
        pages: usize,
        leftover: Vec<String>,
    },
    Saved {
        path: PathBuf,
        original: bool,
    },
    Info(String),
    Error(String),
}

pub struct Editor {
    requests: Sender<EditRequest>,
    events: Receiver<EditEvent>,
}

impl Editor {
    pub fn spawn(path: PathBuf, author: String, wake: impl Fn() + Send + 'static) -> Self {
        let (req_tx, req_rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("vimpdf-editor".into())
            .spawn(move || {
                let send = |e: EditEvent| {
                    let ok = ev_tx.send(e).is_ok();
                    wake();
                    ok
                };
                let mut state = match EditorState::open(&path, author) {
                    Ok(s) => s,
                    Err(e) => {
                        // Not a PDF (or unreadable): annotations unavailable.
                        tracing::warn!("annotations unavailable: {e:#}");
                        send(EditEvent::Loaded(Vec::new()));
                        return;
                    }
                };
                if !send(EditEvent::Loaded(state.read_all())) {
                    return;
                }
                while let Ok(req) = req_rx.recv() {
                    let event = state
                        .handle(req)
                        .unwrap_or_else(|e| EditEvent::Error(format!("{e:#}")));
                    if !send(event) {
                        return;
                    }
                }
                state.cleanup();
            })
            .expect("failed to spawn editor thread");
        Self {
            requests: req_tx,
            events: ev_rx,
        }
    }

    pub fn request(&self, req: EditRequest) {
        if self.requests.send(req).is_err() {
            tracing::error!("editor thread is gone");
        }
    }

    pub fn try_recv(&self) -> Option<EditEvent> {
        self.events.try_recv().ok()
    }
}

/// How to undo one change.
#[derive(Debug, Clone)]
enum Inverse {
    Delete {
        page: usize,
        xref: i32,
    },
    Recreate {
        old_xref: i32,
        annot: NewAnnot,
    },
    SetContents {
        page: usize,
        xref: i32,
        contents: String,
    },
    SetColor {
        page: usize,
        xref: i32,
        color: [u8; 3],
    },
}

struct EditorState {
    doc: PdfDocument,
    original: PathBuf,
    author: String,
    /// Page bounds origins: MuPDF coordinates = page-relative + origin.
    origins: Vec<(f32, f32)>,
    /// Groups of inverse operations, newest last.
    undo: Vec<Vec<Inverse>>,
    /// Annotations recreated by undo get new object numbers.
    remap: HashMap<i32, i32>,
    work_dir: PathBuf,
    work_version: u32,
    working_copies: Vec<PathBuf>,
    unsaved: bool,
    unsaved_other: bool,
}

/// Working copies live in `<temp>/vimpdf-work/<pid>-<n>`: one folder per
/// editing session (each opened document), so sessions never clean up each
/// other's files.
fn work_root() -> PathBuf {
    std::env::temp_dir().join("vimpdf-work")
}

fn new_work_dir() -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    work_root().join(format!("{}-{n}", std::process::id()))
}

/// Remove this process's working copies (on quit: the editor threads may not
/// get to clean up before the process exits).
pub fn remove_own_work_dirs() {
    let prefix = format!("{}-", std::process::id());
    remove_work_dirs(&work_root(), |name, _| name.starts_with(&prefix));
}

/// Remove working copies left by earlier sessions that didn't quit cleanly
/// (crash, kill): folders untouched for longer than `max_age`. A running
/// instance writes to its folder on every edit, so recent ones are kept.
pub fn remove_stale_work_dirs(max_age: std::time::Duration) {
    remove_work_dirs(&work_root(), |_, age| age.is_some_and(|a| a > max_age));
}

fn remove_work_dirs(root: &Path, remove: impl Fn(&str, Option<std::time::Duration>) -> bool) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let age = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok());
        if remove(&name, age) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    // Leave no empty `vimpdf-work` folder behind.
    let _ = std::fs::remove_dir(root);
}

pub fn rgb_to_color(c: [u8; 3]) -> AnnotationColor {
    AnnotationColor::Rgb {
        red: f32::from(c[0]) / 255.0,
        green: f32::from(c[1]) / 255.0,
        blue: f32::from(c[2]) / 255.0,
    }
}

fn color_to_rgb(c: Option<AnnotationColor>) -> [u8; 3] {
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    match c {
        Some(AnnotationColor::Rgb { red, green, blue }) => [to(red), to(green), to(blue)],
        Some(AnnotationColor::Gray(g)) => [to(g); 3],
        Some(AnnotationColor::Cmyk {
            cyan,
            magenta,
            yellow,
            key,
        }) => [
            to((1.0 - cyan) * (1.0 - key)),
            to((1.0 - magenta) * (1.0 - key)),
            to((1.0 - yellow) * (1.0 - key)),
        ],
        None => [255, 235, 59],
    }
}

fn kind_of(t: PdfAnnotationType) -> Option<AnnotKind> {
    match t {
        PdfAnnotationType::Highlight => Some(AnnotKind::Highlight),
        PdfAnnotationType::Text => Some(AnnotKind::Note),
        PdfAnnotationType::Redact => Some(AnnotKind::Redact),
        // Not annotations a reader thinks of as theirs.
        PdfAnnotationType::Link | PdfAnnotationType::Popup | PdfAnnotationType::Widget => None,
        _ => Some(AnnotKind::Other),
    }
}

impl EditorState {
    fn open(path: &Path, author: String) -> Result<Self> {
        let path_str = path.to_str().context("path is not UTF-8")?;
        let doc = PdfDocument::open(path_str).context("not a PDF document")?;
        let count = doc.page_count()?;
        let origins = (0..count)
            .map(|i| {
                let b = doc.load_page(i)?.bounds()?;
                Ok((b.x0, b.y0))
            })
            .collect::<Result<Vec<_>, mupdf::Error>>()?;
        let work_dir = new_work_dir();
        Ok(Self {
            doc,
            original: path.to_owned(),
            author,
            origins,
            undo: Vec::new(),
            remap: HashMap::new(),
            work_dir,
            work_version: 0,
            working_copies: Vec::new(),
            unsaved: false,
            unsaved_other: false,
        })
    }

    fn page(&self, page: usize) -> Result<PdfPage> {
        Ok(self.doc.load_pdf_page(page as i32)?)
    }

    fn resolve(&self, xref: i32) -> i32 {
        let mut x = xref;
        while let Some(&next) = self.remap.get(&x) {
            x = next;
        }
        x
    }

    fn find(&self, page: &PdfPage, xref: i32) -> Result<PdfAnnotation> {
        let xref = self.resolve(xref);
        for a in page.annotations() {
            if a.xref()? == xref {
                return Ok(a);
            }
        }
        bail!("annotation not found (it may have been deleted)")
    }

    fn to_page_rect(&self, page: usize, r: Rect) -> TextRect {
        let (ox, oy) = self.origins[page];
        TextRect {
            x0: r.x0 - ox,
            y0: r.y0 - oy,
            x1: r.x1 - ox,
            y1: r.y1 - oy,
        }
    }

    fn quad(&self, page: usize, r: &TextRect) -> Quad {
        let (ox, oy) = self.origins[page];
        let p = |x: f32, y: f32| Point {
            x: x + ox,
            y: y + oy,
        };
        Quad {
            ul: p(r.x0, r.y0),
            ur: p(r.x1, r.y0),
            ll: p(r.x0, r.y1),
            lr: p(r.x1, r.y1),
        }
    }

    fn info(&self, page: usize, a: &PdfAnnotation) -> Result<Option<AnnotInfo>> {
        let Some(kind) = kind_of(a.r#type()?) else {
            return Ok(None);
        };
        let rects = if a.has_quads().unwrap_or(false) {
            a.quad_points()?
                .iter()
                .map(|q| {
                    let xs = [q.ul.x, q.ur.x, q.ll.x, q.lr.x];
                    let ys = [q.ul.y, q.ur.y, q.ll.y, q.lr.y];
                    let r = Rect {
                        x0: xs.iter().copied().fold(f32::INFINITY, f32::min),
                        y0: ys.iter().copied().fold(f32::INFINITY, f32::min),
                        x1: xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                        y1: ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                    };
                    self.to_page_rect(page, r)
                })
                .collect()
        } else {
            vec![self.to_page_rect(page, a.rect()?)]
        };
        Ok(Some(AnnotInfo {
            page,
            xref: a.xref()?,
            kind,
            color: color_to_rgb(a.color()?),
            rects,
            contents: a.contents()?.unwrap_or_default().to_owned(),
        }))
    }

    fn read_all(&self) -> Vec<AnnotInfo> {
        let mut out = Vec::new();
        for page in 0..self.origins.len() {
            let Ok(p) = self.page(page) else { continue };
            for a in p.annotations() {
                match self.info(page, &a) {
                    Ok(Some(info)) => out.push(info),
                    Ok(None) => {}
                    Err(e) => tracing::warn!("unreadable annotation on page {}: {e:#}", page + 1),
                }
            }
        }
        out
    }

    fn create(&mut self, annot: &NewAnnot) -> Result<Vec<(usize, i32)>> {
        let subtype = match annot.kind {
            AnnotKind::Highlight => PdfAnnotationType::Highlight,
            AnnotKind::Redact => PdfAnnotationType::Redact,
            AnnotKind::Note => PdfAnnotationType::Text,
            AnnotKind::Other => bail!("cannot create this kind of annotation"),
        };
        let mut created = Vec::new();
        for (page, rects) in &annot.parts {
            if rects.is_empty() {
                continue;
            }
            let mut p = self.page(*page)?;
            let mut a = p.create_annotation(subtype)?;
            let quads: Vec<Quad> = rects.iter().map(|r| self.quad(*page, r)).collect();
            if annot.kind == AnnotKind::Note {
                let q = &quads[0];
                a.set_rect(Rect {
                    x0: q.ul.x,
                    y0: q.ul.y,
                    x1: q.ul.x + 18.0,
                    y1: q.ul.y + 18.0,
                })?;
            } else {
                a.set_quad_points(AnnotationQuadPoints::new(quads))?;
            }
            if annot.kind != AnnotKind::Redact {
                a.set_color(rgb_to_color(annot.color))?;
            }
            if !annot.contents.is_empty() {
                a.set_contents(&annot.contents)?;
            }
            a.set_author(&self.author)?;
            a.update()?;
            created.push((*page, a.xref()?));
            p.update()?;
        }
        if created.is_empty() {
            bail!("nothing to annotate");
        }
        Ok(created)
    }

    fn handle(&mut self, req: EditRequest) -> Result<EditEvent> {
        // Whether this change touches something other than redaction marks.
        let other = match &req {
            EditRequest::Create(a) => a.kind != AnnotKind::Redact,
            EditRequest::Delete { page, xref } => {
                let p = self.page(*page)?;
                let a = self.find(&p, *xref)?;
                a.r#type()? != PdfAnnotationType::Redact
            }
            _ => true,
        };
        let (pages, inverse) = match req {
            EditRequest::ApplyRedactions { out } => return self.apply_redactions(&out),
            EditRequest::Save { path } => return self.save(path),
            EditRequest::Undo => {
                let Some(group) = self.undo.pop() else {
                    return Ok(EditEvent::Info("Nothing to undo".into()));
                };
                let mut pages = Vec::new();
                for inv in group.into_iter().rev() {
                    pages.extend(self.apply_inverse(inv)?);
                }
                (pages, None)
            }
            EditRequest::Create(annot) => {
                let created = self.create(&annot)?;
                let pages = created.iter().map(|(p, _)| *p).collect();
                let inv = created
                    .into_iter()
                    .map(|(page, xref)| Inverse::Delete { page, xref })
                    .collect();
                (pages, Some(inv))
            }
            EditRequest::SetContents {
                page,
                xref,
                contents,
            } => {
                let p = self.page(page)?;
                let mut a = self.find(&p, xref)?;
                let old = a.contents()?.unwrap_or_default().to_owned();
                a.set_contents(&contents)?;
                a.update()?;
                let xref = a.xref()?;
                let inv = Inverse::SetContents {
                    page,
                    xref,
                    contents: old,
                };
                (vec![page], Some(vec![inv]))
            }
            EditRequest::SetColor { page, xref, color } => {
                let p = self.page(page)?;
                let mut a = self.find(&p, xref)?;
                let old = color_to_rgb(a.color()?);
                a.set_color(rgb_to_color(color))?;
                a.update()?;
                let xref = a.xref()?;
                let inv = Inverse::SetColor {
                    page,
                    xref,
                    color: old,
                };
                (vec![page], Some(vec![inv]))
            }
            EditRequest::Delete { page, xref } => {
                let inv = self.delete(page, xref)?;
                (vec![page], Some(vec![inv]))
            }
        };
        if let Some(inv) = inverse {
            self.undo.push(inv);
        }
        self.unsaved = true;
        self.unsaved_other |= other;
        let working_copy = self.write_working_copy()?;
        Ok(EditEvent::Changed {
            annots: self.read_all(),
            pages,
            working_copy,
            unsaved: self.unsaved,
            unsaved_other: self.unsaved_other,
        })
    }

    fn delete(&mut self, page: usize, xref: i32) -> Result<Inverse> {
        let mut p = self.page(page)?;
        let a = self.find(&p, xref)?;
        let info = self
            .info(page, &a)?
            .ok_or_else(|| anyhow!("this annotation can't be deleted here"))?;
        p.delete_annotation(a)?;
        p.update()?;
        Ok(Inverse::Recreate {
            old_xref: info.xref,
            annot: NewAnnot {
                kind: info.kind,
                parts: vec![(page, info.rects)],
                color: info.color,
                contents: info.contents,
            },
        })
    }

    fn apply_inverse(&mut self, inv: Inverse) -> Result<Vec<usize>> {
        match inv {
            Inverse::Delete { page, xref } => {
                self.delete(page, xref)?;
                Ok(vec![page])
            }
            Inverse::Recreate { old_xref, annot } => {
                let created = self.create(&annot)?;
                if let Some((_, new)) = created.first() {
                    self.remap.insert(old_xref, *new);
                }
                Ok(created.into_iter().map(|(p, _)| p).collect())
            }
            Inverse::SetContents {
                page,
                xref,
                contents,
            } => {
                let p = self.page(page)?;
                let mut a = self.find(&p, xref)?;
                a.set_contents(&contents)?;
                a.update()?;
                Ok(vec![page])
            }
            Inverse::SetColor { page, xref, color } => {
                let p = self.page(page)?;
                let mut a = self.find(&p, xref)?;
                a.set_color(rgb_to_color(color))?;
                a.update()?;
                Ok(vec![page])
            }
        }
    }

    /// Write the current state to a new working-copy file. A new name each
    /// time: render workers may still be reading the previous one.
    fn write_working_copy(&mut self) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.work_dir)?;
        self.work_version += 1;
        let path = self
            .work_dir
            .join(format!("work-{}.pdf", self.work_version));
        let path_str = path.to_str().context("temp path is not UTF-8")?;
        self.doc
            .save_with_options(path_str, PdfWriteOptions::default())
            .context("cannot write the working copy")?;
        self.working_copies.push(path.clone());
        // Keep the last few: workers switch over lazily.
        while self.working_copies.len() > 3 {
            let old = self.working_copies.remove(0);
            let _ = std::fs::remove_file(old);
        }
        Ok(path)
    }

    fn save(&mut self, path: Option<PathBuf>) -> Result<EditEvent> {
        match path {
            None => {
                let target = self.original.clone();
                let target_str = target.to_str().context("path is not UTF-8")?;
                if self.doc.can_be_saved_incrementally() {
                    let mut opts = PdfWriteOptions::default();
                    opts.set_incremental(true);
                    self.doc
                        .save_with_options(target_str, opts)
                        .with_context(|| format!("cannot save {}", target.display()))?;
                } else {
                    // Write next to it, then replace it in one step.
                    let tmp = target.with_extension("vimpdf-saving.pdf");
                    let tmp_str = tmp.to_str().context("path is not UTF-8")?;
                    self.doc
                        .save_with_options(tmp_str, PdfWriteOptions::default())
                        .with_context(|| format!("cannot write {}", tmp.display()))?;
                    std::fs::rename(&tmp, &target)
                        .with_context(|| format!("cannot replace {}", target.display()))?;
                }
                self.unsaved = false;
                self.unsaved_other = false;
                Ok(EditEvent::Saved {
                    path: target,
                    original: true,
                })
            }
            Some(copy) => {
                let copy_str = copy.to_str().context("path is not UTF-8")?;
                let mut opts = PdfWriteOptions::default();
                opts.set_garbage(true);
                self.doc
                    .save_with_options(copy_str, opts)
                    .with_context(|| format!("cannot write {}", copy.display()))?;
                Ok(EditEvent::Saved {
                    path: copy,
                    original: false,
                })
            }
        }
    }

    /// Apply the redaction marks to a copy of the document and write it to
    /// `out`, then re-read `out` and check that no text is left under any
    /// mark. The open document keeps its marks (nothing is removed from it).
    fn apply_redactions(&mut self, out: &Path) -> Result<EditEvent> {
        use mupdf::pdf::{
            PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions, PdfRedactTextMethod,
        };

        let marks: Vec<(usize, Vec<TextRect>)> = self
            .read_all()
            .into_iter()
            .filter(|a| a.kind == AnnotKind::Redact)
            .map(|a| (a.page, a.rects))
            .collect();
        if marks.is_empty() {
            bail!(
                "No redaction marks: mark text with x in visual mode, an area with X, \
                 or search matches with :redact-matches"
            );
        }
        if out == self.original {
            bail!("Redaction never overwrites the original; give another path");
        }
        let mut pages: Vec<usize> = marks.iter().map(|(p, _)| *p).collect();
        pages.sort_unstable();
        pages.dedup();

        // Work on a separate copy: applying redactions is destructive.
        std::fs::create_dir_all(&self.work_dir)?;
        let src = self.work_dir.join("redact-source.pdf");
        let src_str = src.to_str().context("temp path is not UTF-8")?;
        self.doc
            .save_with_options(src_str, PdfWriteOptions::default())?;
        let copy = PdfDocument::open(src_str)?;
        let options = PdfRedactOptions {
            black_boxes: true,
            image_method: PdfRedactImageMethod::Pixels,
            line_art: PdfRedactLineArtMethod::RemoveIfCovered,
            text: PdfRedactTextMethod::Remove,
        };
        for &page in &pages {
            let mut p = copy.load_pdf_page(page as i32)?;
            p.redact_with_options(options)
                .with_context(|| format!("cannot redact page {}", page + 1))?;
        }
        // A full rewrite with garbage collection: nothing removed may survive
        // in the file (an incremental save would keep the old content).
        let out_str = out.to_str().context("path is not UTF-8")?;
        let mut opts = PdfWriteOptions::default();
        opts.set_incremental(false);
        opts.set_garbage_level(4);
        opts.set_clean(true);
        opts.set_sanitize(true);
        copy.save_with_options(out_str, opts)
            .with_context(|| format!("cannot write {}", out.display()))?;
        drop(copy);
        let _ = std::fs::remove_file(&src);

        // Check the written file.
        let check = crate::document::open(out)?;
        let mut leftover = Vec::new();
        for (page, rects) in &marks {
            let text = crate::document::page_text(&check, *page)?;
            let found: String = text
                .chars
                .iter()
                .filter(|c| !c.c.is_whitespace())
                .filter(|c| {
                    let (cx, cy) = ((c.x0 + c.x1) / 2.0, (c.y0 + c.y1) / 2.0);
                    rects
                        .iter()
                        .any(|r| cx > r.x0 && cx < r.x1 && cy > r.y0 && cy < r.y1)
                })
                .map(|c| c.c)
                .collect();
            if !found.is_empty() {
                leftover.push(format!("page {}: {found:?}", page + 1));
            }
        }
        Ok(EditEvent::Redacted {
            path: out.to_owned(),
            marks: marks.len(),
            pages: pages.len(),
            leftover,
        })
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.work_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> TextRect {
        TextRect { x0, y0, x1, y1 }
    }

    #[test]
    fn work_dirs_are_per_session_and_removed_on_quit() {
        let a = new_work_dir();
        let b = new_work_dir();
        assert_ne!(a, b);
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        // A fresh folder from another process is kept by the stale sweep.
        let other = work_root().join("1-0");
        std::fs::create_dir_all(&other).unwrap();
        remove_stale_work_dirs(std::time::Duration::from_secs(3600));
        assert!(a.exists() && other.exists());
        remove_own_work_dirs();
        assert!(!a.exists() && !b.exists());
        assert!(other.exists(), "other processes' folders are left alone");
        std::fs::remove_dir_all(&other).unwrap();
        let _ = std::fs::remove_dir(work_root());
    }

    /// Print the annotations of the PDF named by `VIMPDF_ANNOT_DUMP`.
    #[test]
    #[ignore = "needs VIMPDF_ANNOT_DUMP"]
    fn dump_annotations() {
        let path = std::env::var("VIMPDF_ANNOT_DUMP").expect("set VIMPDF_ANNOT_DUMP");
        for a in EditorState::open(Path::new(&path), "x".into())
            .unwrap()
            .read_all()
        {
            eprintln!(
                "ANNOT page={} kind={:?} color={:?} rects={} contents={:?}",
                a.page + 1,
                a.kind,
                a.color,
                a.rects.len(),
                a.contents
            );
        }
    }

    /// Redacting a marked word removes it from the written copy and leaves
    /// the original alone.
    #[test]
    fn redaction_removes_text_in_a_copy() {
        let dir = std::env::temp_dir().join(format!("vimpdf-redact-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("doc.pdf");
        std::fs::write(&src, crate::welcome::pdf_bytes()).unwrap();
        let doc = crate::document::open(&src).unwrap();
        let text = crate::document::page_text(&doc, 0).unwrap();
        let (a, b) = text.find("No document", true)[0];
        let rects = text.rects((a, b));
        drop(doc);

        let mut st = EditorState::open(&src, "t".into()).unwrap();
        st.handle(EditRequest::Create(NewAnnot {
            kind: AnnotKind::Redact,
            parts: vec![(0, rects)],
            color: [0, 0, 0],
            contents: String::new(),
        }))
        .unwrap();
        let out = dir.join("doc-redacted.pdf");
        let EditEvent::Redacted {
            marks, leftover, ..
        } = st
            .handle(EditRequest::ApplyRedactions { out: out.clone() })
            .unwrap()
        else {
            panic!("expected Redacted")
        };
        assert_eq!((marks, leftover.len()), (1, 0), "{leftover:?}");

        let red = crate::document::open(&out).unwrap();
        let t = crate::document::page_text(&red, 0).unwrap();
        assert!(t.find("No document", true).is_empty());
        assert!(!t.find("Press ?", true).is_empty(), "other text survives");
        let orig = crate::document::open(&src).unwrap();
        let t = crate::document::page_text(&orig, 0).unwrap();
        assert!(
            !t.find("No document", true).is_empty(),
            "original untouched"
        );
        assert!(
            st.handle(EditRequest::ApplyRedactions { out: src.clone() })
                .is_err()
        );

        st.cleanup();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Full round trip on a real document: create, recolour, comment, undo,
    /// save into a copy, read back.
    #[test]
    fn create_edit_undo_and_save() {
        let dir = std::env::temp_dir().join(format!("vimpdf-annot-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("doc.pdf");
        std::fs::write(&src, crate::welcome::pdf_bytes()).unwrap();

        let mut st = EditorState::open(&src, "tester".into()).unwrap();
        assert!(st.read_all().is_empty());

        let new = NewAnnot {
            kind: AnnotKind::Highlight,
            parts: vec![(0, vec![rect(72.0, 60.0, 200.0, 75.0)])],
            color: [255, 0, 0],
            contents: String::new(),
        };
        let ev = st.handle(EditRequest::Create(new)).unwrap();
        let EditEvent::Changed { annots, pages, .. } = ev else {
            panic!("expected Changed")
        };
        assert_eq!(pages, vec![0]);
        assert_eq!(annots.len(), 1);
        let a = &annots[0];
        assert_eq!((a.kind, a.color), (AnnotKind::Highlight, [255, 0, 0]));
        let r = a.rects[0];
        assert!(
            (r.x0 - 72.0).abs() < 0.5 && (r.y1 - 75.0).abs() < 0.5,
            "{r:?}"
        );

        st.handle(EditRequest::SetColor {
            page: 0,
            xref: a.xref,
            color: [0, 128, 255],
        })
        .unwrap();
        st.handle(EditRequest::SetContents {
            page: 0,
            xref: a.xref,
            contents: "important".into(),
        })
        .unwrap();
        let now = st.read_all();
        assert_eq!(now[0].contents, "important");
        assert_eq!(now[0].color, [0, 128, 255]);

        // Undo the comment, then delete and undo the delete.
        st.handle(EditRequest::Undo).unwrap();
        assert_eq!(st.read_all()[0].contents, "");
        st.handle(EditRequest::Delete {
            page: 0,
            xref: a.xref,
        })
        .unwrap();
        assert!(st.read_all().is_empty());
        st.handle(EditRequest::Undo).unwrap();
        assert_eq!(st.read_all().len(), 1);
        // The recreated annotation has a new number; older undo steps still find it.
        st.handle(EditRequest::Undo).unwrap();
        assert_eq!(st.read_all()[0].color, [255, 0, 0]);

        let copy = dir.join("copy.pdf");
        st.handle(EditRequest::Save {
            path: Some(copy.clone()),
        })
        .unwrap();
        let reread = EditorState::open(&copy, "x".into()).unwrap().read_all();
        assert_eq!(reread.len(), 1);
        assert_eq!(reread[0].color, [255, 0, 0]);

        // Saving into the original keeps it a valid PDF with the annotation.
        st.handle(EditRequest::Save { path: None }).unwrap();
        let reread = EditorState::open(&src, "x".into()).unwrap().read_all();
        assert_eq!(reread.len(), 1);

        st.cleanup();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
