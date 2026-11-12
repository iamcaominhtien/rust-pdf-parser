//! paper2html: convert scientific-paper PDFs (such as those on arXiv) into
//! clean HTML.
//!
//! The pipeline is: PDF access ([`backend`]) -> layout analysis and
//! structuring ([`analysis`]) -> structured document ([`model`]) -> HTML
//! ([`render`]) in one of four fidelity modes. See `ARCHITECTURE.md`.

pub mod analysis;
pub mod backend;
pub mod common;
pub mod model;
pub mod paper;
pub mod render;
pub mod source;

pub use analysis::pipeline::AnalyzeOptions;
pub use paper::{Paper, convert};
pub use render::{HtmlOptions, HtmlOutput, Mode, PageImage};
