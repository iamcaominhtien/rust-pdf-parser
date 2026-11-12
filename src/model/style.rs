//! Style flags of a text run ([`Span::style`](super::Span::style)), combined with `|`.

pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const MONO: u8 = 4;
pub const MATH: u8 = 8;
pub const SUP: u8 = 16;
pub const SUB: u8 = 32;
pub const SANS: u8 = 64;
