//! An opened PDF file: the entry point of the backend, and the only type the
//! rest of the program uses to reach the PDF library.

use super::extract::{RawPage, extract_page};
use super::fonts::{FontInfo, FontRegistry, UNKNOWN_FONT};
use super::render::PageRenderer;
use crate::common::parallel;
use anyhow::{Result, anyhow};
use hayro_interpret::{InterpreterCache, InterpreterSettings};
use hayro_syntax::Pdf;
use std::collections::HashMap;

/// A parsed PDF with at least one page.
pub struct PdfFile {
    /// The parsed file.
    pdf: Pdf,
}

impl PdfFile {
    /// Parses a PDF.
    ///
    /// # Errors
    /// When the data is not a readable PDF, or the PDF has no pages.
    pub fn open(data: Vec<u8>) -> Result<Self> {
        let pdf = Pdf::new(data).map_err(|e| anyhow!("cannot read PDF: {e:?}"))?;
        if pdf.pages().is_empty() {
            return Err(anyhow!("the PDF has no pages"));
        }
        Ok(Self { pdf })
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// A renderer for pictures of pages and page regions. Not thread-safe:
    /// every thread that draws needs its own.
    pub fn renderer(&self) -> PageRenderer<'_> {
        PageRenderer::new(&self.pdf)
    }

    /// Interprets every page, on up to `jobs` threads (0 = one per CPU), and
    /// returns the raw content of the pages with the document's font table
    /// (glyphs refer to fonts by index into it).
    ///
    /// Each worker has its own font registry, so font indices are local to
    /// a worker; they are mapped to one document-wide table here, by font
    /// name and in page order (the same numbering a single pass gives).
    pub fn extract(&self, jobs: usize) -> (Vec<RawPage>, Vec<FontInfo>) {
        let pages = self.pdf.pages();
        let extracted = parallel::map(
            pages.len(),
            jobs,
            || {
                (
                    FontRegistry::new(),
                    InterpreterCache::new(),
                    InterpreterSettings::default(),
                )
            },
            |(fonts, cache, settings), page_idx| {
                let page = extract_page(&pages[page_idx], page_idx, fonts, cache, settings);
                // The fonts this page refers to, by local index.
                let mut used: Vec<(u16, FontInfo)> = Vec::new();
                for glyph in &page.glyphs {
                    if !used.iter().any(|(font, _)| *font == glyph.font) {
                        used.push((glyph.font, fonts.infos[glyph.font as usize].clone()));
                    }
                }
                (page, used)
            },
        );

        let mut infos = FontRegistry::new().infos; // starts with the "unknown" font
        let mut by_name: HashMap<String, u16> = HashMap::new();
        let mut raw = Vec::with_capacity(extracted.len());
        for (mut page, used) in extracted {
            let to_global: HashMap<u16, u16> = used
                .into_iter()
                .map(|(local, info)| {
                    if local == UNKNOWN_FONT {
                        return (local, local);
                    }
                    let global = *by_name.entry(info.name.clone()).or_insert_with(|| {
                        infos.push(info);
                        (infos.len() - 1) as u16
                    });
                    (local, global)
                })
                .collect();
            for glyph in &mut page.glyphs {
                glyph.font = to_global[&glyph.font];
            }
            raw.push(page);
        }
        (raw, infos)
    }
}
