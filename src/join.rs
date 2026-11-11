//! `vimpdf --join a.pdf b.pdf …`: several PDFs shown as one document, in one
//! tab. Their pages are copied, in order, into a new PDF in the temporary
//! folder, which then opens like any other file. Being temporary, it gets
//! the usual protection: `:w` refuses, `:saveas` keeps a permanent copy.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mupdf::document::Location;
use mupdf::link::LinkDestination;
use mupdf::pdf::links::{LinkAction, PdfAction, PdfDestination};
use mupdf::pdf::{PdfDocument, PdfGraftMap, PdfLink, PdfObject, PdfWriteOptions};
use mupdf::{DestinationKind, Outline};

/// Write the pages of `files` (in order) to a new PDF and return its path.
/// Its outline has an entry for each file, holding that file's outline.
pub fn join(files: &[PathBuf]) -> Result<PathBuf> {
    let mut out = PdfDocument::new();
    let mut outline = Vec::new();
    for file in files {
        crate::document::open(file)?; // the usual checks and error messages
        let src = PdfDocument::open(utf8(file)?)
            .with_context(|| format!("'{}' is not a readable PDF", file.display()))?;
        let first = out.page_count().context("cannot count pages")? as u32;
        copy_pages(&src, &mut out)
            .with_context(|| format!("cannot copy the pages of '{}'", file.display()))?;
        let mut down = src.outlines().unwrap_or_default();
        shift_pages(&mut down, first);
        outline.push(Outline {
            title: stem(file),
            uri: None,
            dest: Some(LinkDestination {
                loc: Location {
                    chapter: 0,
                    page_in_chapter: first,
                    page_number: first,
                },
                kind: DestinationKind::Fit,
            }),
            down,
        });
    }
    if let Err(e) = out.set_outlines(&outline) {
        tracing::warn!("cannot write the joined outline: {e}");
    }
    let dir = std::env::temp_dir().join(format!(
        "vimpdf-joined-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let path = dir.join(file_name(files));
    let mut options = PdfWriteOptions::default();
    options
        .set_compress(true)
        .set_compress_fonts(true)
        .set_garbage_level(1);
    out.save_with_options(utf8(&path)?, options)
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

/// The page keys a copy keeps (the inheritable ones are looked up in the
/// page tree, as the copy has no parent to inherit from).
const PAGE_KEYS: [&str; 10] = [
    "Contents",
    "Resources",
    "MediaBox",
    "CropBox",
    "BleedBox",
    "TrimBox",
    "ArtBox",
    "Rotate",
    "UserUnit",
    "Group",
];

/// Annotation keys not copied: links back to the page, to other
/// annotations or to form fields would drag the source's page tree along.
const SKIPPED_ANNOT_KEYS: [&[u8]; 4] = [b"P", b"Parent", b"Popup", b"IRT"];

/// Append the pages of `src` to `out`, with their annotations (but not form
/// fields or popups). One graft map for the whole file, so the fonts and
/// images pages share are copied once. Links are added afterwards, from
/// MuPDF's resolved list, so named destinations still work.
fn copy_pages(src: &PdfDocument, out: &mut PdfDocument) -> Result<()> {
    let mut map = out.new_graft_map()?;
    let first = out.page_count()?;
    let count = src.page_count()?;
    let mut links = Vec::new();
    for i in 0..count {
        let page = src.find_page(i)?;
        let mut copy = out.new_dict()?;
        copy.dict_put("Type", PdfObject::new_name("Page")?)?;
        for key in PAGE_KEYS {
            if let Some(v) = page.get_dict_inheritable(key)? {
                copy.dict_put(key, map.graft_object(&v)?)?;
            }
        }
        let copy = out.add_object(&copy)?;
        let mut annots = out.new_array()?;
        if let Some(src_annots) = page.get_dict("Annots")? {
            for annot in src_annots.array_iter()? {
                if let Some(a) = copy_annot(&annot?, &copy, &mut map, out)? {
                    annots.array_push(a)?;
                }
            }
        }
        if annots.len()? > 0 {
            let mut copy = copy.clone();
            copy.dict_put("Annots", annots)?;
        }
        out.insert_page(first + i, &copy)?;
        links.push(
            src.load_pdf_page(i)?
                .resolved_links()?
                .filter_map(|l| l.ok())
                .filter_map(|l| shift_link(l, first as u32))
                .collect::<Vec<_>>(),
        );
    }
    // Links point at pages, so they go in once all the pages are there.
    for (i, page_links) in links.iter().enumerate() {
        if !page_links.is_empty() {
            let mut page = out.load_pdf_page(first + i as i32)?;
            page.add_links(out, page_links)?;
        }
    }
    Ok(())
}

/// A copy of a source annotation for the page `page` of `out`, or None for
/// the kinds not copied (links are added separately).
fn copy_annot(
    annot: &PdfObject,
    page: &PdfObject,
    map: &mut PdfGraftMap,
    out: &mut PdfDocument,
) -> Result<Option<PdfObject>> {
    let Some(annot) = annot.resolve()? else {
        return Ok(None);
    };
    if !annot.is_dict()? {
        return Ok(None);
    }
    let subtype = annot.get_dict("Subtype")?.and_then(|s| s.as_name().ok());
    if matches!(
        subtype.as_deref(),
        Some(b"Link" | b"Popup" | b"Widget") | None
    ) {
        return Ok(None);
    }
    let mut copy = out.new_dict()?;
    for entry in annot.dict_iter()? {
        let (key, value) = entry?;
        let key = key.as_name()?;
        if SKIPPED_ANNOT_KEYS.contains(&key.as_slice()) {
            continue;
        }
        copy.dict_put(
            String::from_utf8_lossy(&key).into_owned(),
            map.graft_object(&value)?,
        )?;
    }
    copy.dict_put("P", page.clone())?;
    Ok(Some(out.add_object(&copy)?))
}

/// A link with its page moved by `first`, or None for one MuPDF couldn't
/// resolve to a page.
fn shift_link(mut link: PdfLink, first: u32) -> Option<PdfLink> {
    let dest = match &mut link.action {
        LinkAction::Dest(d) | LinkAction::Action(PdfAction::GoTo(d)) => d,
        _ => return Some(link),
    };
    match dest {
        PdfDestination::Page { page, .. } => *page += first,
        PdfDestination::Named(_) => return None,
    }
    Some(link)
}

/// Move an outline's destinations to where its pages are in the joined
/// file (links to other files, by URI, are left alone).
fn shift_pages(items: &mut [Outline], first: u32) {
    for item in items {
        if let Some(dest) = &mut item.dest {
            dest.loc.page_number += first;
            dest.loc.page_in_chapter = dest.loc.page_number;
            dest.loc.chapter = 0;
            item.uri = None;
        }
        shift_pages(&mut item.down, first);
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".into())
}

/// `a + b.pdf`, or `a + 3 more.pdf`: short enough for a tab.
fn file_name(files: &[PathBuf]) -> String {
    match files {
        [] => "joined.pdf".into(),
        [a] => format!("{}.pdf", stem(a)),
        [a, b] => format!("{} + {}.pdf", stem(a), stem(b)),
        [a, rest @ ..] => format!("{} + {} more.pdf", stem(a), rest.len()),
    }
}

fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_joined_file_after_its_parts() {
        let p = |n: &str| PathBuf::from(format!("/x/{n}.pdf"));
        assert_eq!(file_name(&[p("a"), p("b")]), "a + b.pdf");
        assert_eq!(file_name(&[p("a"), p("b"), p("c")]), "a + 2 more.pdf");
    }

    #[test]
    fn joins_the_pages_in_order() {
        let dir = std::env::temp_dir().join(format!("vimpdf-join-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str, pages: usize, width: f32| {
            let mut doc = PdfDocument::new();
            for _ in 0..pages {
                let mut page = doc.new_page((width, 800.0)).unwrap();
                // A note on each page, to check annotations come along.
                let mut note = page
                    .create_annotation(mupdf::pdf::PdfAnnotationType::Text)
                    .unwrap();
                note.set_rect(mupdf::Rect::new(10.0, 10.0, 30.0, 30.0))
                    .unwrap();
                note.set_contents(name).unwrap();
                page.update().unwrap();
            }
            let path = dir.join(name);
            doc.save(path.to_str().unwrap()).unwrap();
            path
        };
        let a = make("a.pdf", 2, 500.0);
        let b = make("b.pdf", 3, 600.0);
        let joined = join(&[a, b]).unwrap();
        assert!(crate::platform::is_temporary(&joined));
        let info = crate::document::DocInfo::load(&joined).unwrap();
        let widths: Vec<f32> = info.page_sizes.iter().map(|s| s.width).collect();
        assert_eq!(widths, [500.0, 500.0, 600.0, 600.0, 600.0]);
        // One outline entry per file, at its first page.
        let (_, outline) = crate::document::load_links_and_outline(&joined).unwrap();
        let entries: Vec<_> = outline
            .iter()
            .map(|o| (o.title.as_str(), o.target.clone()))
            .collect();
        let at = |page| {
            Some(crate::links::LinkTarget::Internal {
                page,
                x: None,
                y: None,
            })
        };
        assert_eq!(entries, [("a", at(0)), ("b", at(2))]);
        let doc = PdfDocument::open(joined.to_str().unwrap()).unwrap();
        let notes: Vec<String> = (0..5)
            .flat_map(|i| {
                let page = doc.load_pdf_page(i).unwrap();
                page.annotations()
                    .map(|a| a.contents().ok().flatten().unwrap_or_default().to_owned())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(notes, ["a.pdf", "a.pdf", "b.pdf", "b.pdf", "b.pdf"]);
        std::fs::remove_dir_all(joined.parent().unwrap()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod real_tests {
    use super::*;

    /// Joins the PDF named by `VIMPDF_TEST_PDF` with itself: twice the
    /// pages and links, and not much more than twice the size. Run with
    /// `VIMPDF_TEST_PDF=paper.pdf cargo test -- --ignored`.
    #[test]
    #[ignore = "needs VIMPDF_TEST_PDF"]
    fn joins_a_real_pdf_with_itself() {
        let path = PathBuf::from(std::env::var("VIMPDF_TEST_PDF").expect("set VIMPDF_TEST_PDF"));
        let joined = join(&[path.clone(), path.clone()]).unwrap();
        let pages = |p: &Path| crate::document::DocInfo::load(p).unwrap().page_sizes.len();
        let links = |p: &Path| {
            let (links, outline) = crate::document::load_links_and_outline(p).unwrap();
            (links.iter().map(Vec::len).sum::<usize>(), outline.len())
        };
        let size = |p: &Path| std::fs::metadata(p).unwrap().len();
        let (l, o) = links(&path);
        let (jl, jo) = links(&joined);
        eprintln!(
            "pages {} -> {}, links {l} -> {jl}, outline {o} -> {jo}, size {} -> {}",
            pages(&path),
            pages(&joined),
            size(&path),
            size(&joined)
        );
        assert_eq!(pages(&joined), 2 * pages(&path));
        assert_eq!(jl, 2 * l);
        assert_eq!(jo, 2 * o + 2);
        assert!(size(&joined) < 3 * size(&path));
        // The second copy's links lead to the second copy, as in the original.
        let n = pages(&path);
        let (orig, _) = crate::document::load_links_and_outline(&path).unwrap();
        let (both, _) = crate::document::load_links_and_outline(&joined).unwrap();
        let target = |l: &crate::links::PageLink| match l.target {
            crate::links::LinkTarget::Internal { page, .. } => Some(page),
            _ => None,
        };
        for (i, page) in orig.iter().enumerate() {
            let shifted: Vec<_> = page.iter().map(|l| target(l).map(|p| p + n)).collect();
            let copied: Vec<_> = both[n + i].iter().map(target).collect();
            assert_eq!(copied, shifted, "links of page {i}");
        }
        // And its pages look the same.
        let render = |p: &Path, page: usize| {
            let size = crate::document::DocInfo::load(p).unwrap().page_sizes[page];
            let whole = crate::document::PixelRect {
                x: 0,
                y: 0,
                w: size.width.ceil() as u32,
                h: size.height.ceil() as u32,
            };
            let mut r = crate::document::PageRenderer::open(p).unwrap();
            r.render(page, 1.0, whole).unwrap().rgb
        };
        for i in [0, n / 2, n - 1] {
            assert!(
                render(&path, i) == render(&joined, n + i),
                "page {i} differs"
            );
        }
        std::fs::remove_dir_all(joined.parent().unwrap()).unwrap();
    }
}
