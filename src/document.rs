//! Thin wrapper over MuPDF. Everything that crosses a thread boundary is plain
//! data: MuPDF objects stay on the thread that created them.

use std::path::Path;

use anyhow::{Context, Result, bail};
use mupdf::{Colorspace, DestinationKind, Device, DisplayList, Document, Matrix, Pixmap, Rect};

use crate::links::{LinkTarget, PageLink};

/// Page size in PDF points, plus the origin of the page's bounds in MuPDF
/// page space (usually 0,0; link and text coordinates are relative to it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
    pub origin_x: f32,
    pub origin_y: f32,
}

impl PageSize {
    #[cfg(test)]
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            origin_x: 0.0,
            origin_y: 0.0,
        }
    }
}

/// Document metadata loaded once at open time.
#[derive(Debug, Clone)]
pub struct DocInfo {
    pub page_sizes: Vec<PageSize>,
}

impl DocInfo {
    pub fn load(path: &Path) -> Result<Self> {
        let doc = open(path)?;
        let count = doc.page_count().context("cannot read page count")?;
        if count <= 0 {
            bail!("'{}' has no pages", path.display());
        }
        let page_sizes = page_sizes(&doc, count)?;
        Ok(Self { page_sizes })
    }

    pub fn page_count(&self) -> usize {
        self.page_sizes.len()
    }
}

fn page_sizes(doc: &Document, count: i32) -> Result<Vec<PageSize>> {
    (0..count)
        .map(|i| {
            let bounds = doc
                .load_page(i)
                .and_then(|p| p.bounds())
                .with_context(|| format!("cannot read page {}", i + 1))?;
            Ok(PageSize {
                width: bounds.x1 - bounds.x0,
                height: bounds.y1 - bounds.y0,
                origin_x: bounds.x0,
                origin_y: bounds.y0,
            })
        })
        .collect()
}

/// Links of every page, in page-relative coordinates. Pages whose links
/// cannot be read get an empty list.
pub fn load_all_links(path: &Path) -> Result<Vec<Vec<PageLink>>> {
    let doc = open(path)?;
    let count = doc.page_count()?;
    let sizes = page_sizes(&doc, count)?;
    let links = (0..count)
        .map(|i| {
            page_links(&doc, i, &sizes).unwrap_or_else(|err| {
                tracing::warn!("cannot read links on page {}: {err:#}", i + 1);
                Vec::new()
            })
        })
        .collect();
    Ok(links)
}

fn page_links(doc: &Document, page: i32, sizes: &[PageSize]) -> Result<Vec<PageLink>> {
    let size = sizes[page as usize];
    let mut out = Vec::new();
    for link in doc.load_page(page)?.links()? {
        let target = match link.dest {
            Some(dest) => {
                let page = dest.loc.page_number as usize;
                let Some(origin) = sizes.get(page) else {
                    continue;
                };
                let (x, y) = match dest.kind {
                    DestinationKind::XYZ { left, top, .. } => (left, top),
                    DestinationKind::FitH { top } | DestinationKind::FitBH { top } => (None, top),
                    DestinationKind::FitV { left } | DestinationKind::FitBV { left } => {
                        (left, None)
                    }
                    DestinationKind::FitR { left, top, .. } => (Some(left), Some(top)),
                    DestinationKind::Fit | DestinationKind::FitB => (None, None),
                };
                LinkTarget::Internal {
                    page,
                    x: x.map(|x| x - origin.origin_x),
                    y: y.map(|y| y - origin.origin_y),
                }
            }
            None if !link.uri.is_empty() => LinkTarget::External(link.uri.clone()),
            None => continue,
        };
        let b = link.bounds;
        out.push(PageLink {
            x0: b.x0 - size.origin_x,
            y0: b.y0 - size.origin_y,
            x1: b.x1 - size.origin_x,
            y1: b.y1 - size.origin_y,
            target,
        });
    }
    Ok(out)
}

pub fn open(path: &Path) -> Result<Document> {
    let path_str = path
        .to_str()
        .with_context(|| format!("path is not valid UTF-8: {}", path.display()))?;
    let doc = Document::open(path_str)
        .with_context(|| format!("'{}' is not a readable PDF", path.display()))?;
    if doc.needs_password().unwrap_or(false) {
        bail!(
            "'{}' is password-protected (not supported yet)",
            path.display()
        );
    }
    Ok(doc)
}

/// A rendered region as tightly packed RGB bytes.
#[derive(Debug)]
pub struct RenderedPage {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

/// A region of a rendered page, in device pixels from the page's top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Display lists kept per renderer: rendering several tiles of one page then
/// interprets the page's content only once.
const DISPLAY_LISTS: usize = 6;

/// Renders regions of pages. Owns its `Document`, so one per thread.
pub struct PageRenderer {
    doc: Document,
    /// Most recently used last.
    lists: Vec<(usize, DisplayList, Rect)>,
}

impl PageRenderer {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            doc: open(path)?,
            lists: Vec::new(),
        })
    }

    fn display_list(&mut self, page: usize) -> Result<&(usize, DisplayList, Rect)> {
        if let Some(i) = self.lists.iter().position(|(p, _, _)| *p == page) {
            let entry = self.lists.remove(i);
            self.lists.push(entry);
        } else {
            let p = self
                .doc
                .load_page(page as i32)
                .with_context(|| format!("cannot load page {}", page + 1))?;
            let bounds = p.bounds()?;
            let list = p.to_display_list(true)?;
            if self.lists.len() == DISPLAY_LISTS {
                self.lists.remove(0);
            }
            self.lists.push((page, list, bounds));
        }
        Ok(self.lists.last().expect("just pushed"))
    }

    /// Render `region` of `page` at `scale` device pixels per PDF point.
    pub fn render(&mut self, page: usize, scale: f32, region: PixelRect) -> Result<RenderedPage> {
        let (_, list, bounds) = self.display_list(page)?;
        // Map the page's top-left corner to device pixel (0, 0).
        let ctm = Matrix::new(
            scale,
            0.0,
            0.0,
            scale,
            -bounds.x0 * scale,
            -bounds.y0 * scale,
        );
        let mut pixmap = Pixmap::new(
            &Colorspace::device_rgb(),
            region.x as i32,
            region.y as i32,
            region.w as i32,
            region.h as i32,
            false,
        )?;
        pixmap.clear_with(255)?;
        {
            let device = Device::from_pixmap(&pixmap)?;
            let area = Rect {
                x0: region.x as f32,
                y0: region.y as f32,
                x1: (region.x + region.w) as f32,
                y1: (region.y + region.h) as f32,
            };
            list.run(&device, &ctm, area).context("rendering failed")?;
            // Dropping the device closes it, flushing all drawing.
        }
        pixmap_to_rgb(&pixmap)
    }
}

fn pixmap_to_rgb(pixmap: &Pixmap) -> Result<RenderedPage> {
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    let n = pixmap.n() as usize;
    let stride = pixmap.stride() as usize;
    if n != 3 {
        bail!("unexpected pixmap format: {n} components");
    }

    let samples = pixmap.samples();
    let row_len = width * 3;
    let rgb = if stride == row_len {
        samples[..row_len * height].to_vec()
    } else {
        let mut out = Vec::with_capacity(row_len * height);
        for row in samples.chunks(stride).take(height) {
            out.extend_from_slice(&row[..row_len]);
        }
        out
    };
    Ok(RenderedPage { width, height, rgb })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders page 1 of the PDF named by `PDFV_TEST_PDF` (PDFs are not
    /// committed). Run with:
    /// `PDFV_TEST_PDF=paper.pdf cargo test -- --ignored`
    /// Set `PDFV_DUMP=out.ppm` to also write the render to disk.
    #[test]
    #[ignore = "needs PDFV_TEST_PDF"]
    fn renders_first_page_of_real_pdf() {
        let path = std::env::var("PDFV_TEST_PDF").expect("set PDFV_TEST_PDF");
        let path = Path::new(&path);
        let info = DocInfo::load(path).unwrap();
        let mut renderer = PageRenderer::open(path).unwrap();

        let scale = 2.0;
        let size = info.page_sizes[0];
        let (w, h) = (
            (size.width * scale).ceil() as u32,
            (size.height * scale).ceil() as u32,
        );
        let whole = PixelRect { x: 0, y: 0, w, h };
        let page = renderer.render(0, scale, whole).unwrap();
        assert_eq!((page.width, page.height), (w as usize, h as usize));
        assert_eq!(page.rgb.len(), page.width * page.height * 3);
        assert!(page.rgb.iter().any(|&b| b < 128), "page is blank");

        // A tile must match the same region of the whole-page render.
        let tile = PixelRect {
            x: 300,
            y: 400,
            w: 200,
            h: 100,
        };
        let part = renderer.render(0, scale, tile).unwrap();
        for row in 0..tile.h as usize {
            let src = ((tile.y as usize + row) * page.width + tile.x as usize) * 3;
            let dst = row * tile.w as usize * 3;
            assert_eq!(
                &page.rgb[src..src + tile.w as usize * 3],
                &part.rgb[dst..dst + tile.w as usize * 3],
                "tile row {row} differs"
            );
        }

        let links = load_all_links(path).unwrap();
        assert_eq!(links.len(), info.page_count());

        if let Ok(out) = std::env::var("PDFV_DUMP") {
            let mut ppm = format!("P6\n{} {}\n255\n", page.width, page.height).into_bytes();
            ppm.extend_from_slice(&page.rgb);
            std::fs::write(out, ppm).unwrap();
        }
    }
}
