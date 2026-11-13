//! High-level entry points: PDF bytes in, structured document and HTML out.

use crate::analysis::pipeline::{Analysis, analyze};
use crate::backend::{PageRenderer, PdfFile};
use crate::config::AnalyzeOptions;
use crate::model::Meta;
use crate::render::{HtmlOptions, HtmlOutput, render};
use anyhow::Result;

/// A loaded PDF.
pub struct Paper {
    /// The PDF itself.
    pdf: PdfFile,
    /// Where the PDF came from (file name, arXiv id, ...), recorded in the output.
    source: Option<String>,
}

impl Paper {
    /// Loads a PDF from its bytes.
    ///
    /// # Errors
    /// When the data is not a readable PDF, or the PDF has no pages.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        Ok(Self {
            pdf: PdfFile::open(data)?,
            source: None,
        })
    }

    /// Records where the PDF came from (file name, arXiv id, ...).
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.pdf.page_count()
    }

    /// Runs content extraction and layout analysis with default options.
    pub fn analyze(&self) -> Analysis {
        self.analyze_with(AnalyzeOptions::default())
    }

    /// Runs content extraction and layout analysis.
    pub fn analyze_with(&self, opts: AnalyzeOptions) -> Analysis {
        let (raw, fonts) = self.pdf.extract(opts.jobs);
        let meta = Meta {
            source: self.source.clone(),
            ..Default::default()
        };
        analyze(raw, &fonts, meta, opts)
    }

    /// A renderer for pictures of pages and page regions.
    pub fn renderer(&self) -> PageRenderer<'_> {
        self.pdf.renderer()
    }

    /// Renders an analysis to HTML in the requested mode.
    pub fn to_html(&self, analysis: &Analysis, opts: &HtmlOptions) -> HtmlOutput {
        render(analysis, &self.pdf, opts)
    }
}

/// One-shot conversion: loads the PDF, analyses it and renders it.
///
/// # Errors
/// When the data is not a readable PDF, or the PDF has no pages.
pub fn convert(data: Vec<u8>, opts: &HtmlOptions) -> Result<HtmlOutput> {
    let paper = Paper::from_bytes(data)?;
    let analysis = paper.analyze();
    Ok(paper.to_html(&analysis, opts))
}
