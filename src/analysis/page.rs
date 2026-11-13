//! Per-page working data and document-wide statistics.

use super::lines::{LinesConfig, build_lines};
use crate::backend::{ImageBox, PathBox, RawPage};
use crate::common::geom::{Rect, weighted_mode};
use crate::model::{Line, RotatedText};
use std::collections::HashMap;

/// Tuning parameters of the document-wide page statistics. Part of [`LayoutConfig`](crate::config::LayoutConfig).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PageConfig {}

pub struct PageData {
    pub index: usize,
    pub width: f32,
    pub height: f32,
    pub lines: Vec<Line>,
    pub rotated: Vec<RotatedText>,
    pub images: Vec<ImageBox>,
    pub paths: Vec<PathBox>,
}

impl PageData {
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

    pub fn rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DocStats {
    /// Font size of the running text.
    pub body_size: f32,
    pub body_font: u16,
    /// Baseline distance of the running text.
    pub leading: f32,
}

pub fn compute_stats(pages: &[PageData], _cfg: &PageConfig) -> DocStats {
    let body_size = weighted_mode(
        pages
            .iter()
            .flat_map(|p| p.lines.iter())
            .map(|l| (l.size, l.char_count() as f32)),
        0.25,
        10.0,
    );
    let mut fonts: HashMap<u16, usize> = HashMap::new();
    for l in pages.iter().flat_map(|p| p.lines.iter()) {
        if (l.size - body_size).abs() < 0.3 {
            *fonts.entry(l.font).or_default() += l.char_count();
        }
    }
    let body_font = fonts.into_iter().max_by_key(|&(_, c)| c).map(|(f, _)| f).unwrap_or(0);

    // Leading: most common baseline distance between vertically adjacent
    // body lines that overlap horizontally.
    let mut gaps: Vec<(f32, f32)> = Vec::new();
    for p in pages {
        let body: Vec<&Line> = p
            .lines
            .iter()
            .filter(|l| (l.size - body_size).abs() < 0.3 && l.char_count() > 20)
            .collect();
        for (i, a) in body.iter().enumerate() {
            for b in body[i + 1..].iter().take(12) {
                let dy = b.baseline - a.baseline;
                if dy > 2.5 * body_size {
                    break;
                }
                if dy > 0.7 * body_size && a.bbox.x_overlap(&b.bbox) > 0.5 * a.bbox.width().min(b.bbox.width()) {
                    gaps.push((dy, 1.0));
                    break;
                }
            }
        }
    }
    let leading = weighted_mode(gaps.into_iter(), 0.1, body_size * 1.2);
    DocStats {
        body_size,
        body_font,
        leading,
    }
}
