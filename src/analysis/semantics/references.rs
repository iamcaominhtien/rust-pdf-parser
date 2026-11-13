//! Splitting reference blocks into one entry each.
//!
//! The block analysis often groups several bibliography entries into one
//! block (their lines are equally spaced). Each entry starts with a label
//! (`[12]`, "12.") at the left edge of the first entry.

use super::patterns::*;
use super::*;

/// Tuning parameters of reference splitting. Part of [`SemanticsConfig`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferencesConfig {
    /// A labelled line starts a new entry when its left edge is at most this
    /// far (multiple of the first line's font size) right of the block's
    /// first line; labels further right are wrapped text such as `[12]`
    /// inside an entry.
    pub entry_indent_tolerance_em: f32,
}

impl Default for ReferencesConfig {
    fn default() -> Self {
        Self {
            entry_indent_tolerance_em: 0.6,
        }
    }
}

/// Splits reference blocks that contain several labelled entries. A block is
/// split only if its first line carries a label; the part after the cut is
/// inserted as a new reference node right after it and is split in turn.
pub(crate) fn split_references(nodes: &mut Vec<Node>, cfg: &SemanticsConfig) {
    let mut i = 0;
    while i < nodes.len() {
        if nodes[i].class == Class::Reference
            && let Some(block) = nodes[i].text_block()
            && let Some(cut) = next_entry_start(block, &cfg.references)
        {
            split_node_at(nodes, i, cut);
        }
        i += 1;
    }
}

/// Index of the first line after the first that starts a new labelled
/// entry. Returns `None` when the block's first line has no label or no
/// later line starts an entry.
fn next_entry_start(block: &TextBlock, cfg: &ReferencesConfig) -> Option<usize> {
    let first_x0 = block.lines[0].bbox.x0;
    let em = block.lines[0].size;
    if !REF_LABEL.is_match(block.lines[0].text().trim()) {
        return None;
    }
    block
        .lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| {
            line.bbox.x0 <= first_x0 + cfg.entry_indent_tolerance_em * em && REF_LABEL.is_match(line.text().trim())
        })
        .map(|(line_idx, _)| line_idx)
}

/// Moves the lines from `cut` on of the text node at `idx` into a new
/// reference node inserted right after it.
///
/// # Panics
///
/// If the node at `idx` is not a [`NodeKind::Text`] node.
fn split_node_at(nodes: &mut Vec<Node>, idx: usize, cut: usize) {
    let NodeKind::Text(block) = &mut nodes[idx].kind else {
        unreachable!()
    };
    let rest_lines = block.lines.split_off(cut);
    let mut rest = block.clone();
    rest.lines = rest_lines;
    let node = Node {
        kind: NodeKind::Text(rest),
        loc: nodes[idx].loc,
        class: Class::Reference,
        tail: nodes[idx].tail,
        leaf_left: nodes[idx].leaf_left,
        leaf_right: nodes[idx].leaf_right,
        strip_prefix: 0,
    };
    nodes.insert(idx + 1, node);
}
