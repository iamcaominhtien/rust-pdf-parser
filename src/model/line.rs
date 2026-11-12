//! A line of text as found on the page, with the position of every glyph.
//! Built by the analysis; the position-preserving renderers draw from it.

use crate::common::geom::Rect;

#[derive(Debug, Clone)]
pub struct LineGlyph {
    pub text: String,
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub size: f32,
    pub font: u16,
    /// 1 = superscript, -1 = subscript, 0 = on the baseline.
    pub script: i8,
    pub space_before: bool,
    pub ink: Rect,
    /// Origin of the glyph as drawn in the PDF (identifies the glyph in the
    /// rendered page).
    pub ox: f32,
    pub oy: f32,
    /// Origin of an accent glyph that was folded into this one.
    pub extra: Option<(f32, f32)>,
    pub color: [u8; 3],
}

#[derive(Debug, Clone)]
pub struct Line {
    pub page: usize,
    /// Nominal box derived from the baseline and the dominant font size.
    pub bbox: Rect,
    /// Union of the glyph outlines.
    pub ink: Rect,
    pub baseline: f32,
    pub size: f32,
    pub font: u16,
    pub glyphs: Vec<LineGlyph>,
}

#[derive(Debug, Clone)]
pub struct RotatedText {
    pub text: String,
    pub bbox: Rect,
    /// Origin of the first glyph, on the baseline.
    pub x: f32,
    pub y: f32,
    /// Direction of the baseline in degrees (y axis pointing down).
    pub angle: f32,
    pub size: f32,
    /// Length along the baseline.
    pub length: f32,
    pub font: u16,
    pub color: [u8; 3],
    /// Origins of all glyphs.
    pub origins: Vec<(f32, f32)>,
}
