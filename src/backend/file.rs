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

/// Endings and beginnings of document-info titles that name the file the
/// PDF was made from rather than the document ("paper.tex", "Microsoft
/// Word - draft.docx"); such a title is not used.
const FILE_NAME_TITLE_MARKS: [&str; 7] = [".tex", ".dvi", ".pdf", ".doc", ".docx", ".indd", "Microsoft Word - "];

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
        let pdf = Pdf::new(data).map_err(|err| anyhow!("cannot read PDF: {err:?}"))?;
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
                let used = fonts_used(&page, fonts);
                (page, used)
            },
        );
        unify_fonts(extracted)
    }

    /// The title in the document information of the PDF, with its
    /// whitespace normalised; `None` when there is none or when it is a
    /// file name (see [`FILE_NAME_TITLE_MARKS`]).
    pub fn info_title(&self) -> Option<String> {
        let raw = self.pdf.metadata().title.as_deref()?;
        let title = decode_text_string(raw).split_whitespace().collect::<Vec<_>>().join(" ");
        let is_file_name = FILE_NAME_TITLE_MARKS
            .iter()
            .any(|mark| title.ends_with(mark) || title.starts_with(mark));
        (!title.is_empty() && !is_file_name).then_some(title)
    }
}

/// A PDF text string as text: UTF-16 when it starts with a byte order mark,
/// UTF-8 when it is valid UTF-8, otherwise PDFDocEncoding (read as Latin-1,
/// which it matches for letters).
fn decode_text_string(raw: &[u8]) -> String {
    let utf16 = |bytes: &[u8], big_endian: bool| {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| {
                if big_endian {
                    u16::from_be_bytes([pair[0], pair[1]])
                } else {
                    u16::from_le_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match raw {
        [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
        _ => {
            std::str::from_utf8(raw).map_or_else(|_| raw.iter().map(|&byte| char::from(byte)).collect(), str::to_string)
        }
    }
}

/// The fonts `page` refers to, as (index in the worker's registry, font
/// description) pairs in order of first use.
fn fonts_used(page: &RawPage, fonts: &FontRegistry<'_>) -> Vec<(u16, FontInfo)> {
    let mut used: Vec<(u16, FontInfo)> = Vec::new();
    for glyph in &page.glyphs {
        if !used.iter().any(|(font, _)| *font == glyph.font) {
            used.push((glyph.font, fonts.infos[glyph.font as usize].clone()));
        }
    }
    used
}

/// Renumbers the worker-local font indices of the extracted pages into one
/// document-wide font table and returns the pages with that table.
///
/// Fonts are identified by name and numbered in order of first use, page by
/// page, so the table does not depend on how pages were spread over
/// workers. Index [`UNKNOWN_FONT`] keeps its meaning.
fn unify_fonts(extracted: Vec<(RawPage, Vec<(u16, FontInfo)>)>) -> (Vec<RawPage>, Vec<FontInfo>) {
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

#[cfg(test)]
mod tests {
    use super::decode_text_string;

    #[test]
    fn text_strings_are_decoded() {
        assert_eq!(decode_text_string(b"Plain title"), "Plain title");
        assert_eq!(decode_text_string(&[0xFE, 0xFF, 0x00, 0x4D, 0x00, 0xE9]), "Mé");
        assert_eq!(decode_text_string(&[0x4D, 0xE9]), "Mé");
        assert_eq!(decode_text_string("Mé".as_bytes()), "Mé");
    }
}
