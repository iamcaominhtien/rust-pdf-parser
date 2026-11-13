//! Adapter around the hayro PDF library: content extraction, fonts and rendering.

pub mod extract;
pub mod file;
pub mod fonts;
pub mod render;

pub use extract::{Dir, Glyph, ImageBox, PathBox, RawPage};
pub use file::PdfFile;
pub use fonts::{FontInfo, FontRegistry};
pub use render::PageRenderer;
