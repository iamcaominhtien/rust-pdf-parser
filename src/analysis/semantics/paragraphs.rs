//! Merging paragraphs split across columns and pages.
//!
//! A paragraph that runs over a column or page break arrives as two blocks:
//! the last flow block of one layout leaf and the first flow block of the
//! next. They are joined when their classes are compatible and the text
//! does not look like a paragraph break (short last line, indented first
//! line, finished sentence). Footnotes continued on the next column or page
//! are joined the same way.

use super::assemble::is_footnote_symbol;
use super::patterns::*;
use super::*;
use crate::analysis::blocks::{LIST_LABEL, TextBlock};
use crate::analysis::page::DocStats;
use crate::model::LineGlyph;

/// Tuning parameters of paragraph merging. Part of [`SemanticsConfig`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParagraphsConfig {
    /// Fewest lines of a paragraph sampled to find out whether the document
    /// indents first lines (shorter ones have no reliable left edge).
    pub indent_sample_min_lines_count: usize,
    /// Paragraphs sampled for the indent must be within this distance
    /// (points) of the body size (no footnote-size or display text).
    pub indent_sample_size_tolerance_pt: f32,
    /// A first line indented by more than this (multiple of the body size)
    /// relative to the other lines is an indented first line.
    pub first_line_indent_min_em: f32,
    /// Fewest sampled paragraphs needed to decide that the document indents
    /// first lines.
    pub indent_min_paragraphs_count: i32,
    /// Share of sampled paragraphs (percent) that must have an indented
    /// first line for the document to count as indenting.
    pub indent_min_percent: i32,
    /// Fewest labelled references (`[3]`, "3.") for the bibliography to
    /// count as labelled; unlabelled pieces then always continue an entry.
    pub labeled_refs_min_count: usize,
    /// Width of an inter-word space (multiple of the font size) used to see
    /// whether the next first word would have fitted on the last line.
    pub word_space_em: f32,
    /// A last line ending more than this (points) before the right edge
    /// (after adding the next first word) is short: the paragraph ended.
    pub short_line_margin_pt: f32,
    /// A first line indented by more than this (multiple of its font size)
    /// relative to the other lines or the column starts a new paragraph.
    pub continuation_indent_min_em: f32,
    /// An unlabelled reference piece indented by more than this (multiple of
    /// its font size) from the column edge is a continuation (hanging
    /// indent).
    pub ref_continuation_indent_min_em: f32,
    /// Lines whose font sizes differ by more than this (points) never belong
    /// to the same paragraph.
    pub size_tolerance_pt: f32,
    /// Footnote pieces whose first lines differ in size by this (points) or
    /// more are not joined.
    pub footnote_size_tolerance_pt: f32,
}

impl Default for ParagraphsConfig {
    fn default() -> Self {
        Self {
            indent_sample_min_lines_count: 3,
            indent_sample_size_tolerance_pt: 0.6,
            first_line_indent_min_em: 0.8,
            indent_min_paragraphs_count: 4,
            indent_min_percent: 35,
            labeled_refs_min_count: 3,
            word_space_em: 0.25,
            short_line_margin_pt: 1.0,
            continuation_indent_min_em: 0.6,
            ref_continuation_indent_min_em: 0.6,
            size_tolerance_pt: 0.4,
            footnote_size_tolerance_pt: 0.4,
        }
    }
}

/// Document-wide conventions that the merge decision depends on.
struct DocumentStyle<'a> {
    /// Fonts of the document (to tell bold glyphs).
    fonts: &'a [FontInfo],
    /// Paragraphs start with an indented first line, so a finished sentence
    /// at a break followed by an unindented line continues the paragraph.
    uses_indent: bool,
    /// Bibliography entries carry labels.
    refs_labeled: bool,
}

/// The two lines that meet at a column or page break, with the measurements
/// the merge rules look at. "Last" is the last line of the earlier block,
/// "first" the first line of the later block.
struct Junction<'a> {
    /// Class of the earlier block.
    prev_class: &'a Class,
    /// Class of the later block.
    next_class: &'a Class,
    /// Left edge of the layout leaf of the later block.
    next_leaf_left: f32,
    /// Last line of the earlier block.
    last: &'a Line,
    /// First line of the later block.
    first: &'a Line,
    /// Font size of `first`.
    em: f32,
    /// Text of `last`.
    last_text: String,
    /// Text of `first`.
    first_text: String,
    /// `last` ends with a hyphen.
    hyphenated: bool,
    /// `last` ends early: the first word of `first` would have fitted.
    short_last: bool,
    /// `first` is indented relative to the rest of its block (or its leaf).
    indented_first: bool,
    /// `first` starts with a list label.
    labeled_first: bool,
    /// `first` starts in bold after a `last` that does not end in bold: a
    /// run-in heading ("**Object Proposals.** There is ...").
    run_in_first: bool,
}

/// Joins paragraphs (and references and list items) split across columns
/// and pages, then footnotes continued on the next column or page.
pub(crate) fn merge_paragraphs(nodes: &mut Vec<Node>, stats: &DocStats, fonts: &[FontInfo], cfg: &SemanticsConfig) {
    let cfg = &cfg.paragraphs;
    let style = DocumentStyle {
        fonts,
        uses_indent: uses_first_line_indent(nodes, stats, cfg),
        refs_labeled: references_are_labeled(nodes, cfg),
    };
    merge_split_blocks(nodes, &style, cfg);
    merge_split_footnotes(nodes, cfg);
}

/// Does this document indent the first line of a paragraph? Looks at the
/// paragraphs of body size with enough lines and requires a minimum share
/// of indented first lines.
fn uses_first_line_indent(nodes: &[Node], stats: &DocStats, cfg: &ParagraphsConfig) -> bool {
    let (mut indented, mut total) = (0, 0);
    for node in nodes {
        if let (Some(block), Class::Paragraph { .. }) = (node.text_block(), &node.class)
            && block.lines.len() >= cfg.indent_sample_min_lines_count
            && (block_size(&block.lines) - stats.body_size).abs() < cfg.indent_sample_size_tolerance_pt
        {
            total += 1;
            let rest = block.lines[1..]
                .iter()
                .map(|line| line.bbox.x0)
                .fold(f32::INFINITY, f32::min);
            if block.lines[0].bbox.x0 > rest + cfg.first_line_indent_min_em * stats.body_size {
                indented += 1;
            }
        }
    }
    total >= cfg.indent_min_paragraphs_count && indented * 100 >= total * cfg.indent_min_percent
}

/// Whether enough reference blocks start with a label.
fn references_are_labeled(nodes: &[Node], cfg: &ParagraphsConfig) -> bool {
    nodes
        .iter()
        .filter(|node| node.class == Class::Reference)
        .filter_map(|node| node.text_block())
        .filter(|block| REF_LABEL.is_match(block_text(&block.lines).trim()))
        .count()
        >= cfg.labeled_refs_min_count
}

/// Joins each mergeable text block with its continuation, repeatedly (a
/// paragraph may span several breaks).
fn merge_split_blocks(nodes: &mut Vec<Node>, style: &DocumentStyle, cfg: &ParagraphsConfig) {
    let mut i = 0;
    while i < nodes.len() {
        match continuation_of(nodes, i, style, cfg) {
            // Stay on `i`: the merged paragraph may continue further.
            Some(j) => append_text_node(nodes, i, j),
            None => i += 1,
        }
    }
}

/// Index of the block that continues the text block at `i`, or `None`.
/// The block at `i` must be the last flow block of its (tail) leaf; the
/// continuation is the next flow block, the first of its own leaf, on the
/// same or the next page, of a compatible class, and the merge rules must
/// accept the junction.
fn continuation_of(nodes: &[Node], i: usize, style: &DocumentStyle, cfg: &ParagraphsConfig) -> Option<usize> {
    let mergeable = matches!(
        nodes[i].class,
        Class::Paragraph { .. } | Class::Reference | Class::ListItem { .. }
    ) && nodes[i].text_block().is_some();
    if !mergeable {
        return None;
    }
    let tail = nodes[i].tail;
    // `i` must be the last flow block of its (tail) leaf.
    let mut j = i + 1;
    while j < nodes.len() && (nodes[j].loc.page, nodes[j].loc.leaf) == tail {
        if is_flow(&nodes[j]) {
            return None;
        }
        j += 1;
    }
    // Next flow block, which must be the first one of its leaf.
    while j < nodes.len() && !is_flow(&nodes[j]) {
        j += 1;
    }
    if j >= nodes.len() || nodes[j].loc.page > tail.0 + 1 {
        return None;
    }
    let next_leaf = (nodes[j].loc.page, nodes[j].loc.leaf);
    let first_in_leaf = !nodes[..j]
        .iter()
        .rev()
        .take_while(|node| (node.loc.page, node.loc.leaf) == next_leaf)
        .any(is_flow);
    if !first_in_leaf || !compatible_classes(&nodes[i].class, &nodes[j].class) || next_leaf == tail {
        return None;
    }
    let junction = Junction::new(&nodes[i], &nodes[j], style.fonts, cfg);
    junction.joins(style, cfg).then_some(j)
}

/// Blocks that take part in the text flow (a paragraph may continue after
/// an equation, but not across a float or footnote).
fn is_flow(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Text(_) | NodeKind::Equation(_))
}

/// Whether a block of class `next` may continue a block of class `prev`.
fn compatible_classes(prev: &Class, next: &Class) -> bool {
    match (prev, next) {
        (Class::Paragraph { in_abstract: a }, Class::Paragraph { in_abstract: b }) => a == b,
        (Class::Reference, Class::Reference) => true,
        (Class::ListItem { .. }, Class::Paragraph { .. }) => true,
        // "... question answering, and" / "(4) a degenerate ...": an
        // enumeration inside a sentence, not a list.
        (Class::Paragraph { .. }, Class::ListItem { bullet: false }) => true,
        _ => false,
    }
}

impl<'a> Junction<'a> {
    /// Measures the break between the text nodes `prev` and `next`.
    ///
    /// # Panics
    ///
    /// If either node is not a text node.
    fn new(prev: &'a Node, next: &'a Node, fonts: &[FontInfo], cfg: &ParagraphsConfig) -> Self {
        let (prev_block, next_block) = (prev.text_block().unwrap(), next.text_block().unwrap());
        let (last, first) = (prev_block.lines.last().unwrap(), &next_block.lines[0]);
        let em = first.size;
        let last_text = last.text();
        let first_text = first.text();
        let right_edge = if prev_block.lines.len() >= 2 {
            prev_block.right
        } else {
            prev.leaf_right
        };
        let hyphenated = last_text.ends_with('-');
        let short_last =
            last.bbox.x1 + cfg.word_space_em * em + first.first_word_width() < right_edge - cfg.short_line_margin_pt;
        let indented_first = if next_block.lines.len() >= 2 {
            let rest = next_block.lines[1..]
                .iter()
                .map(|line| line.bbox.x0)
                .fold(f32::INFINITY, f32::min);
            first.bbox.x0 > rest + cfg.continuation_indent_min_em * em
        } else {
            first.bbox.x0 > next.leaf_left + cfg.continuation_indent_min_em * em
        };
        let labeled_first = LIST_LABEL.is_match(first_text.trim());
        let bold = |glyph: Option<&LineGlyph>| {
            glyph
                .and_then(|glyph| fonts.get(glyph.font as usize))
                .is_some_and(|font| font.bold)
        };
        let visible = |glyph: &&LineGlyph| !glyph.text.trim().is_empty();
        let run_in_first = bold(first.glyphs.iter().find(visible)) && !bold(last.glyphs.iter().rev().find(visible));
        Self {
            prev_class: &prev.class,
            next_class: &next.class,
            next_leaf_left: next.leaf_left,
            last,
            first,
            em,
            last_text,
            first_text,
            hyphenated,
            short_last,
            indented_first,
            labeled_first,
            run_in_first,
        }
    }

    /// Whether the two blocks are one paragraph: the first merge rule that
    /// decides wins; when none does, the document's indent convention
    /// decides (a finished sentence then ends the paragraph only if
    /// paragraphs are not indented).
    fn joins(&self, style: &DocumentStyle, cfg: &ParagraphsConfig) -> bool {
        self.size_rule(cfg)
            .or_else(|| self.reference_rule(style, cfg))
            .or_else(|| self.run_in_rule())
            .or_else(|| self.list_label_rule())
            .or_else(|| self.hyphen_rule())
            .or_else(|| self.layout_break_rule())
            .or_else(|| self.sentence_rule())
            .unwrap_or(style.uses_indent)
    }

    /// Different font sizes: never joined.
    fn size_rule(&self, cfg: &ParagraphsConfig) -> Option<bool> {
        ((self.last.size - self.first.size).abs() > cfg.size_tolerance_pt).then_some(false)
    }

    /// References: an unlabelled piece continues the entry if the
    /// bibliography is labelled or the piece has a hanging indent.
    fn reference_rule(&self, style: &DocumentStyle, cfg: &ParagraphsConfig) -> Option<bool> {
        if *self.prev_class != Class::Reference {
            return None;
        }
        let labeled = REF_LABEL.is_match(self.first_text.trim());
        Some(
            !labeled
                && (style.refs_labeled
                    || self.first.bbox.x0 > self.next_leaf_left + cfg.ref_continuation_indent_min_em * self.em),
        )
    }

    /// A run-in heading starts a new paragraph.
    fn run_in_rule(&self) -> Option<bool> {
        self.run_in_first.then_some(false)
    }

    /// A list label starts a new item if it is a bullet item or the text
    /// before it is finished (sentence end or short line).
    fn list_label_rule(&self) -> Option<bool> {
        (self.labeled_first
            && (matches!(self.next_class, Class::ListItem { bullet: true })
                || ends_terminal(&self.last_text)
                || self.short_last))
            .then_some(false)
    }

    /// A word hyphenated at the break: joined.
    fn hyphen_rule(&self) -> Option<bool> {
        self.hyphenated.then_some(true)
    }

    /// A short last line or an indented first line: a paragraph break.
    fn layout_break_rule(&self) -> Option<bool> {
        (self.short_last || self.indented_first).then_some(false)
    }

    /// An unfinished sentence, or a continuation starting lowercase: joined.
    fn sentence_rule(&self) -> Option<bool> {
        (first_alpha_lower(&self.first_text) || !ends_terminal(&self.last_text)).then_some(true)
    }
}

/// Appends the lines of the text node at `j` to the text node at `i` and
/// removes node `j`; the merged node ends where `j` ended.
///
/// # Panics
///
/// If either node is not a text node.
fn append_text_node(nodes: &mut Vec<Node>, i: usize, j: usize) {
    let removed = nodes.remove(j);
    let NodeKind::Text(appended) = removed.kind else {
        unreachable!()
    };
    let NodeKind::Text(target) = &mut nodes[i].kind else {
        unreachable!()
    };
    target.lines.extend(appended.lines);
    nodes[i].tail = removed.tail;
    nodes[i].leaf_right = removed.leaf_right;
}

/// Footnotes that continue on the next column or page: an unmarked
/// footnote directly following (in footnote order) an unfinished one is
/// appended to it.
fn merge_split_footnotes(nodes: &mut Vec<Node>, cfg: &ParagraphsConfig) {
    let mut last_footnote: Option<usize> = None;
    let mut i = 0;
    while i < nodes.len() {
        let NodeKind::Footnote(block) = &nodes[i].kind else {
            i += 1;
            continue;
        };
        if let Some(prev_idx) = last_footnote
            && continues_footnote(&nodes[prev_idx], &nodes[i], block, cfg)
        {
            let removed = nodes.remove(i);
            let NodeKind::Footnote(appended) = removed.kind else {
                unreachable!()
            };
            let NodeKind::Footnote(target) = &mut nodes[prev_idx].kind else {
                unreachable!()
            };
            target.lines.extend(appended.lines);
            continue;
        }
        last_footnote = Some(i);
        i += 1;
    }
}

/// Whether the footnote `block` of node `node` continues the footnote node
/// `prev`: it is in another leaf, carries no mark while `prev` does, `prev`
/// ends unfinished, and both have the same size.
///
/// # Panics
///
/// If `prev` is not a footnote node.
fn continues_footnote(prev: &Node, node: &Node, block: &TextBlock, cfg: &ParagraphsConfig) -> bool {
    let NodeKind::Footnote(prev_block) = &prev.kind else {
        unreachable!()
    };
    let prev_text = prev_block.lines.last().unwrap().text();
    let different_leaf = (prev.loc.page, prev.loc.leaf) != (node.loc.page, node.loc.leaf);
    different_leaf
        && !has_footnote_mark(block)
        && has_footnote_mark(prev_block)
        && (prev_text.ends_with('-') || !ends_terminal(&prev_text))
        && (prev_block.lines[0].size - block.lines[0].size).abs() < cfg.footnote_size_tolerance_pt
}

/// Whether a footnote starts with its mark (a superscript or a footnote
/// symbol).
fn has_footnote_mark(block: &TextBlock) -> bool {
    block.lines[0].starts_with_superscript()
        || block.lines[0]
            .glyphs
            .first()
            .is_some_and(|glyph| is_footnote_symbol(&glyph.text))
}
