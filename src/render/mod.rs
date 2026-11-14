//! HTML output in four fidelity modes (see [`Mode`]).

mod assets;
mod code;
mod escape;
mod flow;
mod friendly;
mod hard;
mod hard95;
mod mathml;
mod medium;
mod options;
mod table;

pub(crate) use assets::ImageStore;
pub(crate) use escape::*;
pub(crate) use flow::*;
pub(crate) use mathml::{math_css, math_html};
pub use options::{Equations, HtmlOptions, HtmlOutput, Mode, PageImage};
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

/// One `@font-face` rule with the font `data` (woff2) embedded as a base64
/// data URI.
pub(crate) fn font_face(family: &str, style: &str, weight: &str, data: &[u8]) -> String {
    use base64::Engine;
    format!(
        "@font-face{{font-family:\"{family}\";font-style:{style};font-weight:{weight};font-display:swap;\
src:url(data:font/woff2;base64,{}) format(\"woff2\")}}\n",
        base64::engine::general_purpose::STANDARD.encode(data)
    )
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
