//! Style flags of a text run ([`Span::style`](super::Span::style)), combined with `|`.
//!
//! The flags are bits of a `u8` rather than an enum set so that a span's
//! style is cheap to copy, compare and serialise (it appears as a plain
//! number in the JSON output).
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

/// Bold weight (from the font name, weight or descriptor flags).
pub const BOLD: u8 = 1;
/// Italic or oblique shape.
pub const ITALIC: u8 = 2;
/// Monospaced (typewriter) font, rendered as code.
pub const MONO: u8 = 4;
/// Set in a dedicated math font (symbols, math italic, extension glyphs).
pub const MATH: u8 = 8;
/// Raised above the baseline (superscript, footnote marker).
pub const SUP: u8 = 16;
/// Lowered below the baseline (subscript).
pub const SUB: u8 = 32;
/// Sans-serif font.
pub const SANS: u8 = 64;
