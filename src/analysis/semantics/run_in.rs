//! Numbered headings run into the first line of the paragraph below them.
//!
//! Elsevier journals set subsection headings in italics at the start of the
//! paragraph: "*6.2. Background normalisation and control samples* The
//! leading backgrounds ...". Block building sees one paragraph; this stage
//! cuts the heading off, so that it can be classified as a heading and the
//! rest as a paragraph.

use super::patterns::*;
use super::*;
use crate::common::geom::Rect;
use crate::model::LineGlyph;

/// Tuning parameters of the run-in heading split. Part of [`SemanticsConfig`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunInConfig {
    /// Most words of a run-in heading.
    pub max_words_count: usize,
    /// Fewest glyphs of the text after the heading on the same line (a
    /// shorter remainder is an italic word at the end of a line, not a
    /// heading followed by text).
    pub min_rest_glyphs_count: usize,
}

impl Default for RunInConfig {
    fn default() -> Self {
        Self {
            max_words_count: 12,
            min_rest_glyphs_count: 8,
        }
    }
}

/// Cuts every numbered italic run-in heading off the paragraph it starts.
/// The heading becomes a node of its own (class [`Class::Unknown`], to be
/// classified with the others) inserted before the rest of the paragraph.
/// Only nodes still [`Class::Unknown`] are looked at.
pub(crate) fn split_run_in_headings(nodes: &mut Vec<Node>, fonts: &[FontInfo], cfg: &SemanticsConfig) {
    let mut idx = 0;
    while idx < nodes.len() {
        if nodes[idx].class == Class::Unknown
            && let Some(block) = nodes[idx].text_block()
            && let Some(cut) = run_in_cut(block, fonts, &cfg.run_in)
        {
            split_first_line(nodes, idx, cut);
            // The heading node at `idx` is final; go on after it.
            idx += 1;
        }
        idx += 1;
    }
}

/// The glyph index in the first line of `block` at which an italic numbered
/// heading ends and regular text starts, or `None` when the block does not
/// start with such a heading. When the heading fills the whole first line
/// (and the second line is regular text), the index is the number of glyphs
/// of the line.
fn run_in_cut(block: &TextBlock, fonts: &[FontInfo], cfg: &RunInConfig) -> Option<usize> {
    let first = block.lines.first()?;
    let italic = |glyph: &LineGlyph| fonts.get(glyph.font as usize).is_some_and(|font| font.italic);
    let cut = match first.glyphs.iter().position(|glyph| !italic(glyph)) {
        Some(cut) => {
            let rest = &first.glyphs[cut..];
            // The heading is its own word run: a word space precedes the
            // rest, which is regular text of some length that starts a
            // sentence.
            let starts_sentence = rest
                .iter()
                .find(|glyph| glyph.text.chars().any(char::is_alphabetic))
                .is_some_and(|glyph| glyph.text.chars().any(char::is_uppercase));
            let mostly_regular = rest.iter().filter(|glyph| italic(glyph)).count() * 2 <= rest.len();
            (cut > 0
                && rest[0].space_before
                && rest.len() >= cfg.min_rest_glyphs_count
                && starts_sentence
                && mostly_regular)
                .then_some(cut)?
        }
        None => {
            let second = block.lines.get(1)?;
            let second_italic = second.glyphs.iter().filter(|glyph| italic(glyph)).count();
            (second_italic * 2 <= second.glyphs.len()).then_some(first.glyphs.len())?
        }
    };
    let text = glyphs_text(&first.glyphs[..cut]);
    let captures = HEADING_NUM.captures(&text)?;
    let numbered_deep = captures["num"].contains('.') || text.starts_with(&format!("{}.", &captures["num"]));
    (numbered_deep
        && captures["rest"].chars().next().is_some_and(char::is_uppercase)
        && text.split_whitespace().count() <= cfg.max_words_count)
        .then_some(cut)
}

/// The text of a run of glyphs, with a space wherever a glyph follows a
/// word gap.
fn glyphs_text(glyphs: &[LineGlyph]) -> String {
    let mut text = String::new();
    for glyph in glyphs {
        if glyph.space_before && !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&glyph.text);
    }
    text
}

/// Splits the first line of the text node at `idx` after its first `cut`
/// glyphs: the head becomes a one-line node inserted before the node, which
/// keeps the rest of the paragraph.
///
/// # Panics
///
/// If the node at `idx` is not a [`NodeKind::Text`] node.
fn split_first_line(nodes: &mut Vec<Node>, idx: usize, cut: usize) {
    let NodeKind::Text(block) = &mut nodes[idx].kind else {
        unreachable!()
    };
    if cut == block.lines[0].glyphs.len() {
        let mut head_block = block.clone();
        head_block.lines = vec![block.lines.remove(0)];
        head_block.bbox = head_block.lines[0].bbox;
        block.bbox = block.lines.iter().fold(Rect::EMPTY, |acc, line| acc.union(&line.bbox));
        insert_head(nodes, idx, head_block);
        return;
    }
    let mut head = block.lines[0].clone();
    let rest_glyphs = head.glyphs.split_off(cut);
    let head_x1 = head.glyphs.last().map_or(head.bbox.x1, |glyph| glyph.x1);
    head.bbox = Rect::new(head.bbox.x0, head.bbox.y0, head_x1, head.bbox.y1);
    head.ink = ink_of(&head.glyphs);
    let rest = &mut block.lines[0];
    rest.glyphs = rest_glyphs;
    rest.glyphs[0].space_before = false;
    rest.bbox = Rect::new(rest.glyphs[0].x0, rest.bbox.y0, rest.bbox.x1, rest.bbox.y1);
    rest.ink = ink_of(&rest.glyphs);
    let mut head_block = block.clone();
    head_block.lines = vec![head];
    head_block.bbox = head_block.lines[0].bbox;
    block.bbox = block.lines.iter().fold(Rect::EMPTY, |acc, line| acc.union(&line.bbox));
    insert_head(nodes, idx, head_block);
}

/// Inserts a node for the heading block `head_block` before the node at
/// `idx`, with the same position and leaf.
fn insert_head(nodes: &mut Vec<Node>, idx: usize, head_block: TextBlock) {
    let head_node = Node {
        kind: NodeKind::Text(head_block),
        loc: nodes[idx].loc,
        class: Class::Unknown,
        tail: (nodes[idx].loc.page, nodes[idx].loc.leaf),
        leaf_left: nodes[idx].leaf_left,
        leaf_right: nodes[idx].leaf_right,
        strip_prefix: 0,
    };
    nodes.insert(idx, head_node);
}

/// Union of the glyph outlines.
fn ink_of(glyphs: &[LineGlyph]) -> Rect {
    glyphs.iter().fold(Rect::EMPTY, |acc, glyph| acc.union(&glyph.ink))
}

#[cfg(test)]
#[allow(clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;
    use crate::model::Line;

    /// Fonts of the tests: 0 regular, 1 italic.
    fn fonts() -> Vec<FontInfo> {
        vec![
            FontInfo::default(),
            FontInfo {
                italic: true,
                ..Default::default()
            },
        ]
    }

    /// A line made of runs of text, each in the regular (`false`) or the
    /// italic (`true`) font, ten points per glyph.
    fn line_of(parts: &[(&str, bool)], baseline: f32) -> Line {
        let mut glyphs = Vec::new();
        let mut x = 0.0;
        for (text, italic) in parts {
            for (i, ch) in text.chars().enumerate() {
                let space = ch == ' ';
                if space {
                    x += 10.0;
                    continue;
                }
                let after_space = i > 0 && text.chars().nth(i - 1) == Some(' ');
                glyphs.push(LineGlyph {
                    text: ch.to_string(),
                    x0: x,
                    x1: x + 10.0,
                    y: baseline,
                    size: 10.0,
                    font: u16::from(*italic),
                    script: 0,
                    space_before: (after_space || i == 0 && !glyphs.is_empty()) && x > 0.0,
                    ink: Rect::new(x, baseline - 8.0, x + 10.0, baseline),
                    ox: x,
                    oy: baseline,
                    extra: None,
                    color: [0, 0, 0],
                });
                x += 10.0;
            }
        }
        let last = glyphs.last().unwrap().x1;
        Line {
            page: 0,
            bbox: Rect::new(0.0, baseline - 8.0, last, baseline + 2.0),
            ink: Rect::new(0.0, baseline - 8.0, last, baseline),
            baseline,
            size: 10.0,
            font: 0,
            glyphs,
            fixed_pitch: false,
        }
    }

    fn block_of(lines: Vec<Line>) -> TextBlock {
        TextBlock {
            lines,
            loc: Loc::default(),
            left: 0.0,
            right: 500.0,
            centered: false,
            gap_before: 12.0,
            bbox: Rect::new(0.0, 0.0, 500.0, 50.0),
        }
    }

    #[test]
    fn heading_before_text_on_the_same_line_is_cut() {
        let block = block_of(vec![
            line_of(
                &[("6.2. Background samples", true), (" The leading backgrounds", false)],
                10.0,
            ),
            line_of(&[("from SM processes", false)], 22.0),
        ]);
        let cut = run_in_cut(&block, &fonts(), &RunInConfig::default());
        assert_eq!(cut, Some("6.2.Backgroundsamples".chars().count()));
    }

    #[test]
    fn heading_filling_its_line_is_cut() {
        let block = block_of(vec![
            line_of(&[("6.2. Background normalisation", true)], 10.0),
            line_of(&[("The leading backgrounds", false)], 22.0),
        ]);
        assert_eq!(run_in_cut(&block, &fonts(), &RunInConfig::default()), Some(27));
    }

    #[test]
    fn plain_italic_text_is_not_a_heading() {
        let cfg = RunInConfig::default();
        // No number.
        let block = block_of(vec![
            line_of(
                &[("Background samples", true), (" The leading backgrounds", false)],
                10.0,
            ),
            line_of(&[("from SM processes", false)], 22.0),
        ]);
        assert_eq!(run_in_cut(&block, &fonts(), &cfg), None);
        // Regular text only.
        let block = block_of(vec![line_of(&[("6.2. Background samples The leading", false)], 10.0)]);
        assert_eq!(run_in_cut(&block, &fonts(), &cfg), None);
        // Italic emphasis at the end of a numbered sentence: the rest is too short.
        let block = block_of(vec![line_of(
            &[("6.2. Background samples", true), (" The", false)],
            10.0,
        )]);
        assert_eq!(run_in_cut(&block, &fonts(), &cfg), None);
    }

    #[test]
    fn cut_node_keeps_the_rest_of_the_paragraph() {
        let block = block_of(vec![
            line_of(
                &[("6.2. Background samples", true), (" The leading backgrounds", false)],
                10.0,
            ),
            line_of(&[("from SM processes", false)], 22.0),
        ]);
        let cut = run_in_cut(&block, &fonts(), &RunInConfig::default()).unwrap();
        let mut nodes = vec![Node {
            kind: NodeKind::Text(block),
            loc: Loc::default(),
            class: Class::Unknown,
            tail: (0, 0),
            leaf_left: 0.0,
            leaf_right: 500.0,
            strip_prefix: 0,
        }];
        split_first_line(&mut nodes, 0, cut);
        assert_eq!(nodes.len(), 2);
        let head = nodes[0].text_block().unwrap();
        assert_eq!(head.lines.len(), 1);
        assert_eq!(head.lines[0].text(), "6.2. Background samples");
        let rest = nodes[1].text_block().unwrap();
        assert_eq!(rest.lines.len(), 2);
        assert_eq!(rest.lines[0].text(), "The leading backgrounds");
        assert!(rest.lines[0].bbox.x0 > head.lines[0].bbox.x1);
    }
}
