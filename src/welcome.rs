//! A one-page PDF shown when vimpdf starts without a document (e.g. the app
//! was opened from the Dock). Built here so no asset needs to be shipped.

use std::path::PathBuf;

use anyhow::{Context, Result};

const LINES: &[(f32, &str)] = &[
    (28.0, "vimpdf"),
    (13.0, "No document is open."),
    (13.0, ""),
    (13.0, "Open one with   :open <path>"),
    (
        13.0,
        "or drop a PDF on this window, or use Open With in Finder.",
    ),
    (13.0, ""),
    (13.0, "Press ? for the keys, q to quit."),
];

/// The PDF bytes. Offsets in the xref table are computed, so MuPDF reads it
/// without repairs.
pub fn pdf_bytes() -> Vec<u8> {
    let mut content = String::from("BT\n72 770 Td\n");
    for (size, text) in LINES {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        content.push_str(&format!(
            "/F1 {size} Tf\n0 -{} Td\n({escaped}) Tj\n",
            size * 1.6
        ));
    }
    content.push_str("ET\n");

    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];

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

fn path() -> PathBuf {
    crate::platform::private_temp_dir().join("vimpdf-welcome.pdf")
}

/// Whether `path` (canonical) is the welcome page.
pub fn is_welcome(path: &std::path::Path) -> bool {
    std::fs::canonicalize(self::path()).is_ok_and(|p| p == path)
}

/// Write the welcome page to a temporary file and return its path.
pub fn write() -> Result<PathBuf> {
    let path = path();
    std::fs::write(&path, pdf_bytes())
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_page_is_a_readable_pdf_with_its_text() {
        let path =
            std::env::temp_dir().join(format!("vimpdf-welcome-test-{}.pdf", std::process::id()));
        std::fs::write(&path, pdf_bytes()).unwrap();
        let info = crate::document::DocInfo::load(&path).unwrap();
        assert_eq!(info.page_count(), 1);
        let doc = crate::document::open(&path).unwrap();
        let text = crate::document::page_text(&doc, 0).unwrap();
        assert!(!text.find(":open <path>", false).is_empty());
        std::fs::remove_file(&path).unwrap();
    }
}
