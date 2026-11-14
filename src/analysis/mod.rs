//! Layout analysis and structuring: from raw page content to a [`Document`](crate::model::Document).
//!
//! One module per stage, listed in pipeline order; [`pipeline::analyze`]
//! runs them and is the place to read the whole flow:
//!
//! 1. [`page`]: per-page data and document-wide statistics (body size, leading).
//! 2. [`lines`]: glyphs grouped into lines (scripts, accents, rotated text),
//!    while the page data is built.
//! 3. [`furniture`]: repeated running heads, page numbers and margin stamps removed.
//! 4. [`floats`]: figures, tables and algorithms with their captions.
//! 5. [`tables`]: row and column grid of tables (run when the document is
//!    assembled).
//! 6. [`order`]: reading order (columns, zones).
//! 7. [`blocks`]: lines grouped into raw blocks (paragraphs, equations, footnotes).
//! 8. [`semantics`]: classification, paragraph merging, cross-reference links
//!    and the assembly of the document.
//! 9. [`crops`]: pictures of equations made clean (no cut glyphs).
//! 10. [`math`]: the structure of displayed formulas, for MathML.
//!
//! Every stage takes its thresholds from its own struct in
//! [`LayoutConfig`](crate::config::LayoutConfig).

pub mod blocks;
pub mod crops;
pub mod floats;
pub mod furniture;
pub mod lines;
pub mod math;
pub mod order;
pub mod page;
pub mod pipeline;
pub mod semantics;
pub mod tables;
