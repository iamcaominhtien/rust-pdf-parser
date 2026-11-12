//! High-level entry points: PDF bytes in, structured document and HTML out.

use crate::document::{Analysis, analyze};
use crate::html::{HtmlOptions, HtmlOutput, render};
use crate::model::Meta;
use crate::pdf::extract::extract_page;
use crate::pdf::{FontRegistry, PageRenderer};
use anyhow::{Result, anyhow};
use hayro_interpret::{InterpreterCache, InterpreterSettings};
use hayro_syntax::Pdf;

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
        let cache = InterpreterCache::new();
        let settings = InterpreterSettings::default();
        let mut fonts = FontRegistry::new();
        let raw: Vec<_> = self
            .pdf
            .pages()
            .iter()
            .enumerate()
            .map(|(i, p)| extract_page(p, i, &mut fonts, &cache, &settings))
            .collect();
        let meta = Meta {
            source: self.source.clone(),
            ..Default::default()
        };
        analyze(raw, &fonts.infos, meta)
    }

    pub fn renderer(&self) -> PageRenderer<'_> {
        PageRenderer::new(&self.pdf)
    }

    /// Renders an analysis to HTML in the requested mode.
    pub fn to_html(&self, analysis: &Analysis, opts: &HtmlOptions) -> HtmlOutput {
        let renderer = self.renderer();
        render(analysis, &renderer, opts)
    }
}

/// One-shot conversion.
pub fn convert(data: Vec<u8>, opts: &HtmlOptions) -> Result<HtmlOutput> {
    let paper = Paper::from_bytes(data)?;
    let analysis = paper.analyze();
    Ok(paper.to_html(&analysis, opts))
}
