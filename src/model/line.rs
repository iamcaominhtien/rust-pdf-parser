//! A line of text as found on the page, with the position of every glyph.
//! Built by the analysis; the position-preserving renderers draw from it.

use crate::common::geom::Rect;

/// One glyph (or a short run of text from one glyph, such as a ligature
/// expanded to "fi") of a [`Line`], in page coordinates.
#[derive(Debug, Clone)]
pub struct LineGlyph {
    /// The text the glyph stands for, already normalised (ligatures
    /// expanded, special spaces turned into plain spaces).
    pub text: String,
    /// Left edge of the glyph's advance box.
    pub x0: f32,
    /// Right edge of the glyph's advance box (`x0` plus the advance).
    pub x1: f32,
    /// Baseline of the glyph.
    pub y: f32,
    /// Font size in points.
    pub size: f32,
    /// Index into the document's font table.
    pub font: u16,
    /// 1 = superscript, -1 = subscript, 0 = on the baseline.
    pub script: i8,
    /// A word space precedes this glyph (decided from the gap to the
    /// previous glyph; always false for the first glyph of a line).
    pub space_before: bool,
    /// Bounding box of the glyph's outline.
    pub ink: Rect,
    /// Origin of the glyph as drawn in the PDF (identifies the glyph in the
    /// rendered page).
    pub ox: f32,
    /// Vertical coordinate of that origin (see [`LineGlyph::ox`]).
    pub oy: f32,
    /// Origin of an accent glyph that was folded into this one.
    pub extra: Option<(f32, f32)>,
    /// Fill colour as RGB.
    pub color: [u8; 3],
}

/// A horizontal line of text: glyphs that share a baseline, in reading
/// order from left to right.
#[derive(Debug, Clone)]
pub struct Line {
    /// Zero-based page index.
    pub page: usize,
    /// Nominal box derived from the baseline and the dominant font size.
    pub bbox: Rect,
    /// Union of the glyph outlines.
    pub ink: Rect,
    /// Baseline shared by the glyphs that are not raised or lowered.
    pub baseline: f32,
    /// Dominant font size of the line in points.
    pub size: f32,
    /// Dominant font of the line (index into the document's font table).
    pub font: u16,
    /// The glyphs, left to right.
    pub glyphs: Vec<LineGlyph>,
}

/// A run of rotated text (axis labels, the arXiv margin stamp), kept apart
/// from the horizontal lines.
#[derive(Debug, Clone)]
pub struct RotatedText {
    /// The text of the run.
    pub text: String,
    /// Bounding box of the run on the page.
    pub bbox: Rect,
    /// Origin of the first glyph, on the baseline.
    pub x: f32,
    /// Vertical coordinate of that origin (see [`RotatedText::x`]).
    pub y: f32,
    /// Direction of the baseline in degrees (y axis pointing down).
    pub angle: f32,
    /// Font size in points.
    pub size: f32,
    /// Length along the baseline.
    pub length: f32,
    /// Font of the run (index into the document's font table).
    pub font: u16,
    /// Fill colour as RGB.
    pub color: [u8; 3],
    /// Origins of all glyphs.
    pub origins: Vec<(f32, f32)>,
}
