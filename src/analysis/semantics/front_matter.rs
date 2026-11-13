//! Front matter: title, authors, notes and the abstract.

use super::patterns::*;
use super::*;
use crate::analysis::blocks::TextBlock;
use crate::analysis::page::{DocStats, PageData};
use crate::backend::FontInfo;
use crate::common::geom::Rect;
use crate::model::Line;
use crate::model::Loc;
use std::collections::{HashMap, HashSet};

pub(crate) fn classify_front_matter(
    nodes: &mut Vec<Node>,
    pages: &[PageData],
    stats: &DocStats,
    fonts: &[FontInfo],
    cfg: &SemanticsConfig,
) {
    let Some(first_page) = nodes.iter().find(|n| n.text_block().is_some()).map(|n| n.loc.page) else {
        return;
    };
    let page_h = pages[first_page].height;
    let on_first = |n: &Node| n.loc.page == first_page;

    // Title: the largest text in the upper part of the first page.
    let mut tsize = 0.0f32;
    for n in nodes.iter().filter(|n| on_first(n)) {
        if let Some(t) = n.text_block()
            && t.bbox.y0 < 0.65 * page_h
            && block_text(&t.lines).chars().count() >= 4
        {
            tsize = tsize.max(block_size(&t.lines));
        }
    }
    let mut title_range: Option<(usize, usize)> = None;
    if tsize >= stats.body_size + 1.4 {
        for (i, n) in nodes.iter().enumerate() {
            if !on_first(n) {
                continue;
            }
            let is_title = n
                .text_block()
                .is_some_and(|t| block_size(&t.lines) >= tsize - 0.6 && t.bbox.y0 < 0.65 * page_h);
            match (&mut title_range, is_title) {
                (None, true) => title_range = Some((i, i)),
                (Some((_, e)), true) if *e + 1 == i => *e = i,
                _ => {}
            }
        }
    }
    let Some((ts, te)) = title_range else { return };
    for n in nodes[..ts].iter_mut().filter(|n| n.loc.page == first_page) {
        if n.text_block().is_some() {
            n.class = Class::Note;
        }
    }
    for n in &mut nodes[ts..=te] {
        n.class = Class::Title;
    }

    // Authors: everything up to the abstract or the first section.
    let mut stopped_by_prose = false;
    let mut end = te + 1;
    while end < nodes.len() && nodes[end].loc.page == first_page {
        let n = &nodes[end];
        if let Some(t) = n.text_block() {
            let text = block_text(&t.lines);
            let trimmed = text.trim();
            let is_abstract = trimmed.eq_ignore_ascii_case("abstract") || ABSTRACT_INLINE.is_match(trimmed);
            let bold = frac(&t.lines, fonts, |f| f.bold) >= 0.6;
            let is_section = (HEADING_NUM.is_match(trimmed) || HEADING_ROMAN.is_match(trimmed))
                && (bold || block_size(&t.lines) > stats.body_size + 0.5 || upper_ratio(trimmed, cfg) > 0.8)
                && trimmed.chars().count() < 80;
            // Running prose: several justified lines.
            let full = t
                .lines
                .iter()
                .filter(|l| {
                    l.char_count() >= 40
                        && (l.bbox.x1 - t.right).abs() < 1.5
                        && (l.bbox.x0 - t.left).abs() < 2.0 * l.size
                })
                .count();
            let prose = t.lines.len() >= 4 && full >= 3;
            if is_abstract || is_section || prose {
                stopped_by_prose = prose && !is_abstract && !is_section;
                break;
            }
        }
        end += 1;
    }
    // Author blocks are laid out as a grid of small centred groups, which
    // the column analysis cannot order. Re-cluster their lines into one
    // "card" per group and order the cards row by row.
    // An abstract without a heading (journal layouts): the prose that ended
    // the author block, up to the first heading.
    if stopped_by_prose {
        let next_heading = nodes[end..].iter().position(|n| {
            n.loc.page > first_page + 1
                || n.text_block().is_some_and(|t| {
                    let text = block_text(&t.lines);
                    let text = text.trim();
                    text.chars().count() < 80
                        && (HEADING_NUM.is_match(text) || HEADING_ROMAN.is_match(text) || KNOWN_HEADINGS.is_match(text))
                })
        });
        if let Some(k) = next_heading {
            let texts = nodes[end..end + k].iter().filter(|n| n.text_block().is_some()).count();
            if nodes[end + k].loc.page <= first_page + 1 && texts <= 3 {
                for n in nodes[end..end + k].iter_mut().filter(|n| n.text_block().is_some()) {
                    n.class = Class::Paragraph { in_abstract: true };
                }
            }
        }
    }

    // Besides the nodes before the stop, anything that lies between the
    // title and the stop geometrically belongs to the author block (the
    // column analysis may have ordered a second author column later).
    let title_bottom = nodes[ts..=te]
        .iter()
        .filter_map(|n| n.text_block())
        .map(|t| t.bbox.y1)
        .fold(0.0, f32::max);
    let stop_y = nodes
        .get(end)
        .filter(|n| n.loc.page == first_page)
        .and_then(|n| n.text_block())
        .map(|t| t.bbox.y0)
        .unwrap_or(f32::INFINITY);
    let author_idx: Vec<usize> = (te + 1..nodes.len())
        .filter(|&i| {
            let n = &nodes[i];
            let Some(t) = n.text_block() else { return false };
            n.loc.page == first_page
                && n.class == Class::Unknown
                && (i < end || (stop_y.is_finite() && t.bbox.y1 <= stop_y - 1.0 && t.bbox.y0 >= title_bottom - 2.0))
        })
        .collect();
    if author_idx.is_empty() {
        return;
    }
    let author_zones: HashSet<usize> = author_idx.iter().map(|&i| nodes[i].loc.zone).collect();
    let mut lines: Vec<Line> = Vec::new();
    for &i in &author_idx {
        lines.extend(nodes[i].text_block().unwrap().lines.iter().cloned());
    }
    lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
    let mut parent: Vec<usize> = (0..lines.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        p[i] = r;
        r
    }
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            let (a, b) = (&lines[i], &lines[j]);
            let gap = b.bbox.y0 - a.bbox.y1;
            if gap > 0.9 * a.size.max(b.size) {
                break;
            }
            let same_row = (a.baseline - b.baseline).abs() < 2.0;
            if !same_row && a.bbox.x_overlap(&b.bbox) >= 0.5 * a.bbox.width().min(b.bbox.width()) {
                let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                if ra != rb {
                    parent[rb] = ra;
                }
            }
        }
    }
    let mut cards: Vec<(Rect, Vec<Line>)> = Vec::new();
    let mut card_of: HashMap<usize, usize> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let r = find(&mut parent, i);
        let ci = *card_of.entry(r).or_insert_with(|| {
            cards.push((Rect::EMPTY, Vec::new()));
            cards.len() - 1
        });
        cards[ci].0 = cards[ci].0.union(&line.bbox);
        cards[ci].1.push(line.clone());
    }
    // Rows of cards.
    cards.sort_by(|a, b| a.0.y0.total_cmp(&b.0.y0));
    let mut ordered: Vec<(Rect, Vec<Line>)> = Vec::new();
    let mut row: Vec<(Rect, Vec<Line>)> = Vec::new();
    let mut row_box = Rect::EMPTY;
    for c in cards {
        if !row.is_empty() && row_box.y_overlap(&c.0) < 0.4 * row_box.height().min(c.0.height()) {
            row.sort_by(|a, b| a.0.x0.total_cmp(&b.0.x0));
            ordered.append(&mut row);
            row_box = Rect::EMPTY;
        }
        row_box = row_box.union(&c.0);
        row.push(c);
    }
    row.sort_by(|a, b| a.0.x0.total_cmp(&b.0.x0));
    ordered.append(&mut row);

    let template_loc = Loc {
        zone_cols: 1,
        col: 0,
        zone: usize::MAX / 2,
        ..nodes[author_idx[0]].loc
    };
    let (leaf_left, leaf_right) = (nodes[author_idx[0]].leaf_left, nodes[author_idx[0]].leaf_right);
    let new_nodes: Vec<Node> = ordered
        .into_iter()
        .map(|(bbox, lines)| Node {
            kind: NodeKind::Text(TextBlock {
                lines,
                loc: template_loc,
                left: bbox.x0,
                right: bbox.x1,
                centered: true,
                gap_before: f32::INFINITY,
                bbox,
            }),
            loc: template_loc,
            class: Class::Authors,
            tail: (template_loc.page, template_loc.leaf),
            leaf_left,
            leaf_right,
            strip_prefix: 0,
        })
        .collect();
    // Replace the old author nodes (keeping floats etc. in between).
    let first = author_idx[0];
    let drop: HashSet<usize> = author_idx.iter().copied().collect();
    let mut i = 0;
    nodes.retain(|_| {
        let keep = !drop.contains(&i);
        i += 1;
        keep
    });
    nodes.splice(first..first, new_nodes);

    // Whatever else shared a zone with the author grid (typically the
    // "Abstract" heading) is not really part of a multi-column zone.
    for z in author_zones {
        let others: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.loc.page == first_page && n.loc.zone == z && n.class != Class::Authors)
            .map(|(i, _)| i)
            .collect();
        if others.len() <= 3 {
            for i in others {
                nodes[i].loc.zone_cols = 1;
                nodes[i].loc.col = 0;
            }
        }
    }
}
