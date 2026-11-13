//! paper2html: convert scientific-paper PDFs (such as those on arXiv) into
//! clean HTML.
//!
//! The pipeline is: PDF access ([`backend`]) -> layout analysis and
//! structuring ([`analysis`]) -> structured document ([`model`]) -> HTML
//! ([`render`]) in one of four fidelity modes. See `ARCHITECTURE.md`.
//!
//! The entry points are [`Paper`] (load, analyse and render step by step,
//! e.g. to produce several modes from one analysis) and [`convert`] (all in
//! one call with default options). Analysis thresholds live in
//! [`LayoutConfig`], reachable through [`AnalyzeOptions`].

pub mod analysis;
pub mod backend;
pub mod common;
pub mod config;
pub mod model;
pub mod paper;
pub mod render;
pub mod source;

pub use config::{AnalyzeOptions, LayoutConfig};
pub use paper::{Paper, convert};
pub use render::{HtmlOptions, HtmlOutput, Mode, PageImage};
