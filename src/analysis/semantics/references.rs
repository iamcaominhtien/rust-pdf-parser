//! Splitting reference blocks into one entry each.

use super::patterns::*;
use super::*;

/// Splits reference blocks that contain several labelled entries.
pub(crate) fn split_references(nodes: &mut Vec<Node>) {
    let mut i = 0;
    while i < nodes.len() {
        if nodes[i].class != Class::Reference {
            i += 1;
            continue;
        }
        let Some(t) = nodes[i].text_block() else {
            i += 1;
            continue;
        };
        let first_x0 = t.lines[0].bbox.x0;
        let em = t.lines[0].size;
        if !REF_LABEL.is_match(t.lines[0].text().trim()) {
            i += 1;
            continue;
        }
        let cut = t
            .lines
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, l)| l.bbox.x0 <= first_x0 + 0.6 * em && REF_LABEL.is_match(l.text().trim()));
        let Some((k, _)) = cut else {
            i += 1;
            continue;
        };
        let NodeKind::Text(t) = &mut nodes[i].kind else {
            unreachable!()
        };
        let rest_lines = t.lines.split_off(k);
        let mut rest = t.clone();
        rest.lines = rest_lines;
        let node = Node {
            kind: NodeKind::Text(rest),
            loc: nodes[i].loc,
            class: Class::Reference,
            tail: nodes[i].tail,
            leaf_left: nodes[i].leaf_left,
            leaf_right: nodes[i].leaf_right,
            strip_prefix: 0,
        };
        nodes.insert(i + 1, node);
        i += 1;
    }
}
