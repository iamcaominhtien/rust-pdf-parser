//! Structuring: turns the per-page blocks into a document. Each block
//! becomes a [`Node`]; the stages classify nodes (title, authors, headings,
//! paragraphs, list items, references), split and merge them, and
//! [`assemble`] builds the final [`Document`](crate::model::Document).
//!
//! The stages run in this order (see [`crate::analysis::pipeline`]):
//! [`classify_front_matter`], [`classify_body`], [`split_references`],
//! [`merge_paragraphs`] and finally [`build_document`]. Their thresholds
//! are collected in [`SemanticsConfig`].
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

#[allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)] // TODO(semantics2): remove once the file is cleaned up
pub(crate) mod assemble;
#[allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)] // TODO(semantics2): remove once the file is cleaned up
pub(crate) mod body;
#[allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)] // TODO(semantics2): remove once the file is cleaned up
pub(crate) mod front_matter;
#[allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)] // TODO(semantics2): remove once the file is cleaned up
pub(crate) mod links;
#[allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)] // TODO(semantics2): remove once the file is cleaned up
pub(crate) mod paragraphs;
#[allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)] // TODO(semantics2): remove once the file is cleaned up
pub(crate) mod patterns;
#[allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)] // TODO(semantics2): remove once the file is cleaned up
pub(crate) mod references;
#[allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)] // TODO(semantics2): remove once the file is cleaned up
pub(crate) mod text;

pub use body::BodyConfig;

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

/// Font size, in points, reported by [`block_size`] for a block without
/// lines. Blocks always have lines, so this only keeps the function total.
const FALLBACK_SIZE_PT: f32 = 10.0;

/// Tuning parameters of the structuring stages. Part of [`LayoutConfig`](crate::config::LayoutConfig).
///
/// The top-level fields are used by the helpers shared by several stages
/// (case and small-caps detection); the stage-specific thresholds are in the
/// nested structs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SemanticsConfig {
    /// Fewest letters a text needs before its share of capitals is
    /// measured; shorter texts ("A", "IoT") report no capitals at all, so
    /// that acronyms and initials do not look like all-caps headings.
    pub upper_ratio_min_letters_count: usize,
    /// Fewest letters a block needs to count as set in small caps.
    pub small_caps_min_letters_count: i32,
    /// Two capitals whose sizes differ by less than this (points) are of the
    /// same size; small caps need at least two distinct sizes.
    pub small_caps_size_tolerance_pt: f32,
    /// Fewest distinct capital sizes for small caps (full capitals plus the
    /// reduced ones).
    pub small_caps_min_sizes_count: usize,
    /// Classification of headings, paragraphs, list items and references.
    pub body: BodyConfig,
}

impl Default for SemanticsConfig {
    fn default() -> Self {
        Self {
            upper_ratio_min_letters_count: 4,
            small_caps_min_letters_count: 4,
            small_caps_size_tolerance_pt: 0.2,
            small_caps_min_sizes_count: 2,
            body: BodyConfig::default(),
        }
    }
}

/// What a node has been recognised as. Starts as [`Class::Unknown`] and is
/// set by the classification stages; [`build_document`] maps it to the
/// [`BlockKind`](crate::model::BlockKind) of the output block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Class {
    /// Not classified (yet). Text left unknown is output as a paragraph.
    Unknown,
    /// Part of the paper's title.
    Title,
    /// One card (group of lines) of the author block.
    Authors,
    /// Text on the first page above the title (journal header, notes).
    Note,
    /// A section heading.
    Heading {
        /// Nesting level, 1 for top-level sections.
        level: u8,
        /// The section number as printed ("3.2", "IV", "A.1"), if any.
        number: Option<String>,
    },
    /// Running text.
    Paragraph {
        /// The paragraph belongs to the abstract.
        in_abstract: bool,
    },
    /// An item of a bulleted or numbered list.
    ListItem {
        /// The label is a bullet symbol rather than a number or letter.
        bullet: bool,
    },
    /// One entry of the bibliography.
    Reference,
}

/// The content of a node: the raw block it was made from.
pub(crate) enum NodeKind {
    /// A block of text lines.
    Text(TextBlock),
    /// A displayed formula.
    Equation(EquationBlock),
    /// A figure, table or algorithm with its caption.
    Float(Float),
    /// A footnote at the bottom of a page.
    Footnote(TextBlock),
}

/// A block in reading order, with what the classification stages found out
/// about it.
pub(crate) struct Node {
    /// The block's content.
    pub(crate) kind: NodeKind,
    /// Position of the (first line of the) block in the page layout.
    pub(crate) loc: Loc,
    /// What the block has been recognised as.
    pub(crate) class: Class,
    /// Leaf of the last line (differs from `loc` after merging).
    pub(crate) tail: (usize, usize),
    /// Left edge of the layout leaf (column) the block starts in.
    pub(crate) leaf_left: f32,
    /// Right edge of the layout leaf the block ends in.
    pub(crate) leaf_right: f32,
    /// Number of leading characters to drop from the text (inline labels).
    pub(crate) strip_prefix: usize,
}

impl Node {
    /// The text block of a [`NodeKind::Text`] node; `None` for every other
    /// kind (footnotes included).
    pub(crate) fn text_block(&self) -> Option<&TextBlock> {
        match &self.kind {
            NodeKind::Text(block) => Some(block),
            _ => None,
        }
    }
}

/// Plain text of a block: the text of its lines joined by single spaces.
pub(crate) fn block_text(lines: &[Line]) -> String {
    lines.iter().map(|line| line.text()).collect::<Vec<_>>().join(" ")
}

/// Typical font size of a block: the median of its line sizes, or
/// [`FALLBACK_SIZE_PT`] when it has no lines.
pub(crate) fn block_size(lines: &[Line]) -> f32 {
    let sizes: Vec<f32> = lines.iter().map(|line| line.size).collect();
    median(&sizes, FALLBACK_SIZE_PT)
}

/// Fraction (0–1) of the glyphs of `lines` whose font satisfies `pred`
/// (e.g. bold), weighted by the number of glyphs per line. Returns 0 when
/// there are no glyphs.
pub(crate) fn frac(lines: &[Line], fonts: &[FontInfo], pred: impl Fn(&FontInfo) -> bool + Copy) -> f32 {
    let n: usize = lines.iter().map(|line| line.glyphs.len()).sum();
    if n == 0 {
        return 0.0;
    }
    let matching: f32 = lines
        .iter()
        .map(|line| line.font_frac(fonts, pred) * line.glyphs.len() as f32)
        .sum();
    matching / n as f32
}

/// Share (0–1) of capitals among the letters of `text`. Texts with fewer
/// than [`SemanticsConfig::upper_ratio_min_letters_count`] letters return 0.
pub(crate) fn upper_ratio(text: &str, cfg: &SemanticsConfig) -> f32 {
    let letters: Vec<char> = text.chars().filter(|ch| ch.is_alphabetic()).collect();
    if letters.len() < cfg.upper_ratio_min_letters_count {
        return 0.0;
    }
    letters.iter().filter(|ch| ch.is_uppercase()).count() as f32 / letters.len() as f32
}

/// Whether the lines are set in small caps: every letter glyph is a capital
/// and the capitals come in at least two sizes (full and reduced). Returns
/// `false` as soon as a lowercase letter is seen.
pub(crate) fn small_caps(lines: &[Line], cfg: &SemanticsConfig) -> bool {
    let mut sizes: Vec<f32> = Vec::new();
    let mut letters = 0;
    for glyph in lines.iter().flat_map(|line| line.glyphs.iter()) {
        if glyph.text.chars().all(|ch| ch.is_alphabetic()) {
            letters += 1;
            if !glyph.text.chars().all(|ch| ch.is_uppercase()) {
                return false;
            }
            if !sizes
                .iter()
                .any(|size| (size - glyph.size).abs() < cfg.small_caps_size_tolerance_pt)
            {
                sizes.push(glyph.size);
            }
        }
    }
    letters >= cfg.small_caps_min_letters_count && sizes.len() >= cfg.small_caps_min_sizes_count
}

/// Whether the first letter of `text` is lowercase (`false` when it has no
/// letters). A paragraph continuing a sentence typically starts lowercase.
pub(crate) fn first_alpha_lower(text: &str) -> bool {
    text.chars()
        .find(|ch| ch.is_alphabetic())
        .is_some_and(|ch| ch.is_lowercase())
}

/// Whether `text` ends a sentence (`.`, `?`, `!` or `:`), ignoring closing
/// brackets and quotes after the punctuation.
pub(crate) fn ends_terminal(text: &str) -> bool {
    let trimmed = text.trim_end().trim_end_matches([')', '”', '’', '"', '\'', ']']);
    trimmed.ends_with(['.', '?', '!', ':'])
}
