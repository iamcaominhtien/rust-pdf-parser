//! Adapter around the hayro PDF library: content extraction, fonts and rendering.
//!
//! This is the only part of the program that knows about hayro: everything
//! else works on the plain data it returns ([`RawPage`], [`FontInfo`]) and
//! draws through [`PageRenderer`].
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

pub mod extract;
pub mod file;
pub mod fonts;
pub mod render;

pub use extract::{Dir, Glyph, ImageBox, PathBox, RawPage};
pub use file::PdfFile;
pub use fonts::{FontInfo, FontRegistry};
pub use render::PageRenderer;
