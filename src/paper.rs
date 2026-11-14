//! High-level entry points: PDF bytes in, structured document and HTML out.

use crate::analysis::pipeline::{Analysis, analyze};
use crate::backend::{PageRenderer, PdfFile};
use crate::config::AnalyzeOptions;
use crate::model::Meta;
use crate::render::{HtmlOptions, HtmlOutput, render};
use anyhow::Result;

/// A loaded PDF, ready to be analysed and rendered.
///
/// Typical use: [`Paper::from_bytes`], [`Paper::analyze`], then
/// [`Paper::to_html`] once per output mode (the analysis does not depend on
/// the mode, so it is computed once).
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

    /// Records where the PDF came from (file name, arXiv id, ...); it ends up
    /// in [`Meta::source`].
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
    ///
    /// Pages are read on `opts.jobs` threads; the result does not depend on
    /// the number of threads.
    pub fn analyze_with(&self, opts: AnalyzeOptions) -> Analysis {
        let (raw, fonts) = self.pdf.extract(opts.jobs);
        let meta = Meta {
            source: self.source.clone(),
            ..Default::default()
        };
        let mut analysis = analyze(raw, &fonts, meta, opts);
        // A cover that is a picture holds no title text: the document
        // information of the PDF names the document then.
        if analysis.doc.meta.title.is_none() {
            analysis.doc.meta.title = self.pdf.info_title();
        }
        analysis
    }

    /// A renderer for pictures of pages and page regions. Not thread-safe:
    /// every thread that draws needs its own.
    pub fn renderer(&self) -> PageRenderer<'_> {
        self.pdf.renderer()
    }

    /// Renders an analysis to HTML in the requested mode. `analysis` must
    /// come from this paper, since images are drawn from its PDF.
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
