//! paper2html: convert scientific-paper PDFs (such as those on arXiv) into
//! clean HTML.
//!
//! The pipeline is: PDF content extraction ([`pdf`]) -> layout analysis
//! ([`layout`]) -> structured document ([`document`], [`model`]) -> HTML
//! ([`html`]) in one of three fidelity modes.

pub mod arxiv;
pub mod convert;
pub mod document;
pub mod geom;
pub mod html;
pub mod layout;
pub mod model;
pub mod pdf;

pub use convert::{Paper, convert};
pub use html::{HtmlOptions, HtmlOutput, Mode, PageImage};
