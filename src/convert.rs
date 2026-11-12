//! High-level entry points: PDF bytes in, structured document and HTML out.

use crate::document::{Analysis, AnalyzeOptions, analyze};
use crate::html::{HtmlOptions, HtmlOutput, render};
use crate::model::Meta;
use crate::par;
use crate::pdf::extract::{RawPage, extract_page};
use crate::pdf::fonts::FontInfo;
use crate::pdf::{FontRegistry, PageRenderer};
use anyhow::{Result, anyhow};
use hayro_interpret::{InterpreterCache, InterpreterSettings};
use hayro_syntax::Pdf;
use std::collections::HashMap;

/// A loaded PDF.
pub struct Paper {
    pdf: Pdf,
    source: Option<String>,
}

impl Paper {
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        let pdf = Pdf::new(data).map_err(|e| anyhow!("cannot read PDF: {e:?}"))?;
        if pdf.pages().is_empty() {
            return Err(anyhow!("the PDF has no pages"));
        }
        Ok(Self { pdf, source: None })
    }

    /// Records where the PDF came from (file name, arXiv id, ...).
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Runs content extraction and layout analysis.
    pub fn analyze(&self) -> Analysis {
        self.analyze_with(AnalyzeOptions::default())
    }

    pub fn analyze_with(&self, opts: AnalyzeOptions) -> Analysis {
        let (raw, fonts) = self.extract(opts.jobs);
        let meta = Meta {
            source: self.source.clone(),
            ..Default::default()
        };
        analyze(raw, &fonts, meta, opts)
    }

    /// Interprets every page, on up to `jobs` threads (0 = one per CPU).
    ///
    /// Each worker has its own font registry, so font indices are local to
    /// a worker; they are mapped to one document-wide table here, by font
    /// name and in page order (the same numbering a single pass gives).
    fn extract(&self, jobs: usize) -> (Vec<RawPage>, Vec<FontInfo>) {
        let pages = self.pdf.pages();
        let extracted = par::map(
            pages.len(),
            jobs,
            || {
                (
                    FontRegistry::new(),
                    InterpreterCache::new(),
                    InterpreterSettings::default(),
                )
            },
            |(fonts, cache, settings), i| {
                let page = extract_page(&pages[i], i, fonts, cache, settings);
                // The fonts this page refers to, by local index.
                let mut used: Vec<(u16, FontInfo)> = Vec::new();
                for g in &page.glyphs {
                    if !used.iter().any(|(f, _)| *f == g.font) {
                        used.push((g.font, fonts.infos[g.font as usize].clone()));
                    }
                }
                (page, used)
            },
        );

        let mut infos = FontRegistry::new().infos; // starts with the "unknown" font
        let mut by_name: HashMap<String, u16> = HashMap::new();
        let mut raw = Vec::with_capacity(extracted.len());
        for (mut page, used) in extracted {
            let map: HashMap<u16, u16> = used
                .into_iter()
                .map(|(local, info)| {
                    if local == crate::pdf::fonts::UNKNOWN_FONT {
                        return (local, local);
                    }
                    let global = *by_name.entry(info.name.clone()).or_insert_with(|| {
                        infos.push(info);
                        (infos.len() - 1) as u16
                    });
                    (local, global)
                })
                .collect();
            for g in &mut page.glyphs {
                g.font = map[&g.font];
            }
            raw.push(page);
        }
        (raw, infos)
    }

    pub fn renderer(&self) -> PageRenderer<'_> {
        PageRenderer::new(&self.pdf)
    }

    /// Renders an analysis to HTML in the requested mode.
    pub fn to_html(&self, analysis: &Analysis, opts: &HtmlOptions) -> HtmlOutput {
        render(analysis, &self.pdf, opts)
    }
}

/// One-shot conversion.
pub fn convert(data: Vec<u8>, opts: &HtmlOptions) -> Result<HtmlOutput> {
    let paper = Paper::from_bytes(data)?;
    let analysis = paper.analyze();
    Ok(paper.to_html(&analysis, opts))
}
