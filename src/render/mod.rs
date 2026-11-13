//! HTML output in four fidelity modes (see [`Mode`]).

mod assets;
mod escape;
mod flow;
mod friendly;
mod hard;
mod hard95;
mod medium;
mod options;
mod table;

pub(crate) use assets::ImageStore;
pub(crate) use escape::*;
pub(crate) use flow::*;
pub use options::{HtmlOptions, HtmlOutput, Mode, PageImage};
pub(crate) use table::*;

use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::model::*;

/// One output mode: turns an analysed document into a complete HTML page.
pub(crate) trait Renderer {
    /// Writes the HTML. Pictures (page backgrounds, figures, equations) are
    /// requested from `images`, which returns their `src` and draws them
    /// afterwards.
    fn render(&self, analysis: &Analysis, images: &mut ImageStore<'_>) -> String;
}

/// The renderer of a mode: one implementation of [`Renderer`] per [`Mode`].
fn renderer_for(mode: Mode) -> &'static dyn Renderer {
    match mode {
        Mode::Friendly => &friendly::Friendly,
        Mode::Medium => &medium::Medium,
        Mode::Hard => &hard::Hard,
        Mode::Hard95 => &hard95::Hard95,
    }
}

/// Renders an analysed document as HTML in the mode chosen in `opts`.
///
/// The HTML is written first, with placeholders for the pictures; the
/// pictures are then drawn from `pdf` (on `opts.jobs` threads) and either
/// inlined as data URIs or returned in [`HtmlOutput::files`].
pub fn render(analysis: &Analysis, pdf: &PdfFile, opts: &HtmlOptions) -> HtmlOutput {
    let mut images = ImageStore { opts, jobs: Vec::new() };
    let html = renderer_for(opts.mode).render(analysis, &mut images);
    images.finish(pdf, html)
}
