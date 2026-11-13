//! Merging paragraphs split across columns and pages.

use super::patterns::*;
use super::*;
use crate::analysis::blocks::{LIST_LABEL, TextBlock};
use crate::analysis::page::DocStats;

pub(crate) fn merge_paragraphs(nodes: &mut Vec<Node>, stats: &DocStats, _cfg: &SemanticsConfig) {
    // Does this document indent the first line of a paragraph?
    let (mut indented, mut total) = (0, 0);
    for n in nodes.iter() {
        if let (Some(t), Class::Paragraph { .. }) = (n.text_block(), &n.class)
            && t.lines.len() >= 3
            && (block_size(&t.lines) - stats.body_size).abs() < 0.6
        {
            total += 1;
            let rest = t.lines[1..].iter().map(|l| l.bbox.x0).fold(f32::INFINITY, f32::min);
            if t.lines[0].bbox.x0 > rest + 0.8 * stats.body_size {
                indented += 1;
            }
        }
    }
    let uses_indent = total >= 4 && indented * 100 >= total * 35;
    let refs_labeled = nodes
        .iter()
        .filter(|n| n.class == Class::Reference)
        .filter_map(|n| n.text_block())
        .filter(|t| REF_LABEL.is_match(block_text(&t.lines).trim()))
        .count()
        >= 3;

    let flow = |n: &Node| matches!(n.kind, NodeKind::Text(_) | NodeKind::Equation(_));
    let mut i = 0;
    while i < nodes.len() {
        let mergeable = matches!(
            nodes[i].class,
            Class::Paragraph { .. } | Class::Reference | Class::ListItem { .. }
        ) && nodes[i].text_block().is_some();
        if !mergeable {
            i += 1;
            continue;
        }
        let tail = nodes[i].tail;
        // `i` must be the last flow block of its (tail) leaf.
        let mut j = i + 1;
        let mut last_in_leaf = true;
        while j < nodes.len() && (nodes[j].loc.page, nodes[j].loc.leaf) == tail {
            if flow(&nodes[j]) {
                last_in_leaf = false;
                break;
            }
            j += 1;
        }
        if !last_in_leaf {
            i += 1;
            continue;
        }
        // Next flow block, which must be the first one of its leaf.
        while j < nodes.len() && !flow(&nodes[j]) {
            j += 1;
        }
        if j >= nodes.len() || nodes[j].loc.page > tail.0 + 1 {
            i += 1;
            continue;
        }
        let jleaf = (nodes[j].loc.page, nodes[j].loc.leaf);
        let first_in_leaf = !nodes[..j]
            .iter()
            .rev()
            .take_while(|n| (n.loc.page, n.loc.leaf) == jleaf)
            .any(flow);
        let compatible = match (&nodes[i].class, &nodes[j].class) {
            (Class::Paragraph { in_abstract: a }, Class::Paragraph { in_abstract: b }) => a == b,
            (Class::Reference, Class::Reference) => true,
            (Class::ListItem { .. }, Class::Paragraph { .. }) => true,
            // "... question answering, and" / "(4) a degenerate ...": an
            // enumeration inside a sentence, not a list.
            (Class::Paragraph { .. }, Class::ListItem { bullet: false }) => true,
            _ => false,
        };
        if !first_in_leaf || !compatible || jleaf == tail {
            i += 1;
            continue;
        }
        let decision = {
            let (p1, p2) = (nodes[i].text_block().unwrap(), nodes[j].text_block().unwrap());
            let (a, b) = (p1.lines.last().unwrap(), &p2.lines[0]);
            let em = b.size;
            let a_text = a.text();
            let b_text = b.text();
            let right_edge = if p1.lines.len() >= 2 {
                p1.right
            } else {
                nodes[i].leaf_right
            };
            let hyph = a_text.ends_with('-');
            let short_a = a.bbox.x1 + 0.25 * em + b.first_word_width() < right_edge - 1.0;
            let indent_b = if p2.lines.len() >= 2 {
                let rest = p2.lines[1..].iter().map(|l| l.bbox.x0).fold(f32::INFINITY, f32::min);
                b.bbox.x0 > rest + 0.6 * em
            } else {
                b.bbox.x0 > nodes[j].leaf_left + 0.6 * em
            };
            let label_b = LIST_LABEL.is_match(b_text.trim());
            if (a.size - b.size).abs() > 0.4 {
                false
            } else if nodes[i].class == Class::Reference {
                let labeled = REF_LABEL.is_match(b_text.trim());
                !labeled && (refs_labeled || b.bbox.x0 > nodes[j].leaf_left + 0.6 * em)
            } else if label_b
                && (matches!(nodes[j].class, Class::ListItem { bullet: true }) || ends_terminal(&a_text) || short_a)
            {
                false
            } else if hyph {
                true
            } else if short_a || indent_b {
                false
            } else if first_alpha_lower(&b_text) || !ends_terminal(&a_text) {
                true
            } else {
                uses_indent
            }
        };
        if !decision {
            i += 1;
            continue;
        }
        let removed = nodes.remove(j);
        let NodeKind::Text(t2) = removed.kind else {
            unreachable!()
        };
        let NodeKind::Text(t1) = &mut nodes[i].kind else {
            unreachable!()
        };
        t1.lines.extend(t2.lines);
        nodes[i].tail = removed.tail;
        nodes[i].leaf_right = removed.leaf_right;
        // Stay on `i`: the merged paragraph may continue further.
    }

    // Footnotes that continue on the next column or page: an unmarked
    // footnote directly following (in footnote order) an unfinished one.
    let marked = |t: &TextBlock| {
        t.lines[0].starts_with_superscript()
            || t.lines[0]
                .glyphs
                .first()
                .is_some_and(|g| matches!(g.text.as_str(), "∗" | "†" | "‡" | "§" | "¶" | "*"))
    };
    let mut last_fn: Option<usize> = None;
    let mut i = 0;
    while i < nodes.len() {
        let NodeKind::Footnote(t) = &nodes[i].kind else {
            i += 1;
            continue;
        };
        if let Some(p) = last_fn {
            let NodeKind::Footnote(prev) = &nodes[p].kind else {
                unreachable!()
            };
            let prev_text = prev.lines.last().unwrap().text();
            let different_leaf = (nodes[p].loc.page, nodes[p].loc.leaf) != (nodes[i].loc.page, nodes[i].loc.leaf);
            if different_leaf
                && !marked(t)
                && marked(prev)
                && (prev_text.ends_with('-') || !ends_terminal(&prev_text))
                && (prev.lines[0].size - t.lines[0].size).abs() < 0.4
            {
                let removed = nodes.remove(i);
                let NodeKind::Footnote(t2) = removed.kind else {
                    unreachable!()
                };
                let NodeKind::Footnote(t1) = &mut nodes[p].kind else {
                    unreachable!()
                };
                t1.lines.extend(t2.lines);
                continue;
            }
        }
        last_fn = Some(i);
        i += 1;
    }
}
