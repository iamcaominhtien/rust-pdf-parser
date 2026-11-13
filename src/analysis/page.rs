//! Per-page working data and document-wide statistics.
//!
//! [`PageData`] is the mutable working copy of one page that the analysis
//! stages pass along; [`DocStats`] holds the few document-wide measurements
//! (body font size, body font, line pitch) that most later thresholds are
//! expressed relative to.

use super::lines::{LinesConfig, build_lines};
use crate::backend::{ImageBox, PathBox, RawPage};
use crate::common::geom::{Rect, weighted_mode};
use crate::model::{Line, RotatedText};
use std::collections::HashMap;

/// Tuning parameters of the document-wide page statistics. Part of [`LayoutConfig`](crate::config::LayoutConfig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageConfig {
    /// Bin width (in points) of the font-size histogram whose mode is the
    /// body size. A quarter point merges sizes that differ only by rounding
    /// in the PDF while keeping 9 pt and 10 pt apart.
    pub body_size_bin_pt: f32,
    /// Body size (in points) assumed when the document has no text at all.
    pub default_body_size_pt: f32,
    /// Largest difference (in points) between a line's size and the body
    /// size for the line to count as running text.
    pub body_size_tolerance_pt: f32,
    /// Lines with at most this many glyphs are ignored when measuring the
    /// leading: short lines (headings, equation numbers, labels) are often
    /// not part of a regular paragraph.
    pub leading_min_glyphs_count: usize,
    /// How many following body lines are inspected when looking for the
    /// next line below a given one.
    pub leading_lookahead_count: usize,
    /// A vertical distance above this (in body sizes) ends the search for
    /// the line below: anything further away is a paragraph or section gap,
    /// not the line pitch.
    pub leading_max_gap_em: f32,
    /// A vertical distance must exceed this (in body sizes) to count as a
    /// line pitch; smaller distances are lines of a neighbouring column or
    /// fragments on (nearly) the same baseline.
    pub leading_min_gap_em: f32,
    /// Minimum horizontal overlap of two lines, as a fraction of the
    /// narrower one, for them to count as vertically adjacent lines of the
    /// same column.
    pub leading_overlap_ratio: f32,
    /// Bin width (in points) of the histogram of line pitches.
    pub leading_bin_pt: f32,
    /// Leading (in body sizes) assumed when no pair of adjacent body lines
    /// was found; 1.2 is the usual default line spacing of typesetters.
    pub default_leading_em: f32,
}

impl Default for PageConfig {
    fn default() -> Self {
        Self {
            body_size_bin_pt: 0.25,
            default_body_size_pt: 10.0,
            body_size_tolerance_pt: 0.3,
            leading_min_glyphs_count: 20,
            leading_lookahead_count: 12,
            leading_max_gap_em: 2.5,
            leading_min_gap_em: 0.7,
            leading_overlap_ratio: 0.5,
            leading_bin_pt: 0.1,
            default_leading_em: 1.2,
        }
    }
}

/// The working data of one page: its size, its text lines (built from the
/// glyphs), rotated text runs, images and vector paths. Later stages remove
/// or rewrite lines in place.
pub struct PageData {
    /// Zero-based page number in the document.
    pub index: usize,
    /// Page width in points.
    pub width: f32,
    /// Page height in points.
    pub height: f32,
    /// Horizontal text lines, sorted by baseline then left edge.
    pub lines: Vec<Line>,
    /// Text runs that are not horizontal (rotated margin stamps, axis labels).
    pub rotated: Vec<RotatedText>,
    /// Raster images drawn on the page.
    pub images: Vec<ImageBox>,
    /// Bounding boxes of vector paths (rules, figure strokes, table borders).
    pub paths: Vec<PathBox>,
}

impl PageData {
    /// Builds the working data of a page from what the backend extracted,
    /// grouping the glyphs into lines with `lines_cfg`.
    pub fn from_raw(raw: RawPage, lines_cfg: &LinesConfig) -> Self {
        let (lines, rotated) = build_lines(&raw, lines_cfg);
        Self {
            index: raw.index,
            width: raw.width,
            height: raw.height,
            lines,
            rotated,
            images: raw.images,
            paths: raw.paths,
        }
    }

    /// The rectangle covering the whole page.
    pub fn rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }
}

/// Document-wide measurements of the running text.
#[derive(Debug, Clone, Copy)]
pub struct DocStats {
    /// Font size of the running text.
    pub body_size: f32,
    /// Font index (into the document's font table) used by most running text.
    pub body_font: u16,
    /// Baseline distance of the running text.
    pub leading: f32,
}

/// Measures the body size, body font and leading of the document.
///
/// The body size is the glyph-weighted most common line size; the body font
/// is the font with most glyphs among lines of that size; the leading is the
/// most common baseline distance between adjacent body lines. Each falls
/// back to a default (see [`PageConfig`]) when the document has no text.
pub fn compute_stats(pages: &[PageData], cfg: &PageConfig) -> DocStats {
    let body_size = measure_body_size(pages, cfg);
    let body_font = find_body_font(pages, body_size, cfg);
    let leading = measure_leading(pages, body_size, cfg);
    DocStats {
        body_size,
        body_font,
        leading,
    }
}

/// Most common line size, each line weighted by its number of glyphs.
fn measure_body_size(pages: &[PageData], cfg: &PageConfig) -> f32 {
    weighted_mode(
        pages
            .iter()
            .flat_map(|page| page.lines.iter())
            .map(|line| (line.size, line.char_count() as f32)),
        cfg.body_size_bin_pt,
        cfg.default_body_size_pt,
    )
}

/// Font with the most glyphs among the lines set in the body size, or 0 when
/// there are no such lines.
fn find_body_font(pages: &[PageData], body_size: f32, cfg: &PageConfig) -> u16 {
    let mut fonts: HashMap<u16, usize> = HashMap::new();
    for line in pages.iter().flat_map(|page| page.lines.iter()) {
        if (line.size - body_size).abs() < cfg.body_size_tolerance_pt {
            *fonts.entry(line.font).or_default() += line.char_count();
        }
    }
    fonts
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(font, _)| font)
        .unwrap_or(0)
}

/// Leading: most common baseline distance between vertically adjacent body
/// lines that overlap horizontally.
fn measure_leading(pages: &[PageData], body_size: f32, cfg: &PageConfig) -> f32 {
    let mut gaps: Vec<(f32, f32)> = Vec::new();
    for page in pages {
        let body: Vec<&Line> = page
            .lines
            .iter()
            .filter(|line| {
                (line.size - body_size).abs() < cfg.body_size_tolerance_pt
                    && line.char_count() > cfg.leading_min_glyphs_count
            })
            .collect();
        for (i, upper) in body.iter().enumerate() {
            for lower in body[i + 1..].iter().take(cfg.leading_lookahead_count) {
                let dy = lower.baseline - upper.baseline;
                if dy > cfg.leading_max_gap_em * body_size {
                    break;
                }
                if dy > cfg.leading_min_gap_em * body_size
                    && upper.bbox.x_overlap(&lower.bbox)
                        > cfg.leading_overlap_ratio * upper.bbox.width().min(lower.bbox.width())
                {
                    gaps.push((dy, 1.0));
                    break;
                }
            }
        }
    }
    weighted_mode(gaps.into_iter(), cfg.leading_bin_pt, body_size * cfg.default_leading_em)
}
