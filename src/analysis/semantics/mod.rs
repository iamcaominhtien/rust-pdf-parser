//! Structuring: turns the per-page blocks into a document. Each block
//! becomes a [`Node`]; the stages classify nodes (title, authors, headings,
//! paragraphs, list items, references), split and merge them, and
//! [`assemble`] builds the final [`Document`](crate::model::Document).

pub(crate) mod assemble;
pub(crate) mod body;
pub(crate) mod front_matter;
pub(crate) mod links;
pub(crate) mod paragraphs;
pub(crate) mod patterns;
pub(crate) mod references;
pub(crate) mod text;

pub(crate) use assemble::build_document;
pub(crate) use body::classify_body;
pub(crate) use front_matter::classify_front_matter;
pub(crate) use paragraphs::merge_paragraphs;
pub(crate) use references::split_references;

use crate::analysis::blocks::{EquationBlock, TextBlock};
use crate::analysis::floats::Float;
use crate::backend::FontInfo;
use crate::common::geom::median;
use crate::model::Line;
use crate::model::Loc;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Class {
    Unknown,
    Title,
    Authors,
    Note,
    Heading { level: u8, number: Option<String> },
    Paragraph { in_abstract: bool },
    ListItem { bullet: bool },
    Reference,
}

pub(crate) enum NodeKind {
    Text(TextBlock),
    Equation(EquationBlock),
    Float(Float),
    Footnote(TextBlock),
}

pub(crate) struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) loc: Loc,
    pub(crate) class: Class,
    /// Leaf of the last line (differs from `loc` after merging).
    pub(crate) tail: (usize, usize),
    pub(crate) leaf_left: f32,
    pub(crate) leaf_right: f32,
    /// Number of leading characters to drop from the text (inline labels).
    pub(crate) strip_prefix: usize,
}

impl Node {
    pub(crate) fn text_block(&self) -> Option<&TextBlock> {
        match &self.kind {
            NodeKind::Text(t) => Some(t),
            _ => None,
        }
    }
}

pub(crate) fn block_text(lines: &[Line]) -> String {
    lines.iter().map(|l| l.text()).collect::<Vec<_>>().join(" ")
}

pub(crate) fn block_size(lines: &[Line]) -> f32 {
    let sizes: Vec<f32> = lines.iter().map(|l| l.size).collect();
    median(&sizes, 10.0)
}

pub(crate) fn frac(lines: &[Line], fonts: &[FontInfo], pred: impl Fn(&FontInfo) -> bool + Copy) -> f32 {
    let n: usize = lines.iter().map(|l| l.glyphs.len()).sum();
    if n == 0 {
        return 0.0;
    }
    let k: f32 = lines
        .iter()
        .map(|l| l.font_frac(fonts, pred) * l.glyphs.len() as f32)
        .sum();
    k / n as f32
}

pub(crate) fn upper_ratio(s: &str) -> f32 {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() < 4 {
        return 0.0;
    }
    letters.iter().filter(|c| c.is_uppercase()).count() as f32 / letters.len() as f32
}

/// Small caps are set as capitals of two sizes.
pub(crate) fn small_caps(lines: &[Line]) -> bool {
    let mut sizes: Vec<f32> = Vec::new();
    let mut letters = 0;
    for g in lines.iter().flat_map(|l| l.glyphs.iter()) {
        if g.text.chars().all(|c| c.is_alphabetic()) {
            letters += 1;
            if !g.text.chars().all(|c| c.is_uppercase()) {
                return false;
            }
            if !sizes.iter().any(|s| (s - g.size).abs() < 0.2) {
                sizes.push(g.size);
            }
        }
    }
    letters >= 4 && sizes.len() >= 2
}

pub(crate) fn first_alpha_lower(s: &str) -> bool {
    s.chars().find(|c| c.is_alphabetic()).is_some_and(|c| c.is_lowercase())
}

pub(crate) fn ends_terminal(s: &str) -> bool {
    let t = s.trim_end().trim_end_matches([')', '”', '’', '"', '\'', ']']);
    t.ends_with(['.', '?', '!', ':'])
}
