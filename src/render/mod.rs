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
use crate::model::*;
use hayro_syntax::Pdf;

pub fn render(analysis: &Analysis, pdf: &Pdf, opts: &HtmlOptions) -> HtmlOutput {
    let mut images = ImageStore { opts, jobs: Vec::new() };
    let html = match opts.mode {
        Mode::Friendly => friendly::render(&analysis.doc, &mut images),
        Mode::Medium => medium::render(&analysis.doc, &mut images),
        Mode::Hard => hard::render(analysis, &mut images),
        Mode::Hard95 => hard95::render(analysis, &mut images),
    };
    images.finish(pdf, html)
}
