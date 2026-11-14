//! Front matter: title, authors, notes and the abstract.
//!
//! Works on the first page that has text: the title is the run of largest
//! text in its upper part; text above it is a note; the author block runs
//! from the title to the abstract, the first section heading or the first
//! piece of running prose. The author block is re-ordered as a grid of
//! "cards" (one per author or affiliation group), and an abstract without a
//! heading is recognised when prose ends the author block.

use super::patterns::*;
use super::*;
use crate::analysis::blocks::TextBlock;
use crate::analysis::page::{DocStats, PageData};
use crate::backend::FontInfo;
use crate::common::geom::Rect;
use crate::model::Line;
use crate::model::Loc;
use std::collections::{HashMap, HashSet};

/// Zone given to the re-ordered author cards: a value no layout zone has,
/// so that the cards form a zone of their own.
const AUTHOR_ZONE: usize = usize::MAX / 2;

/// Tuning parameters of front-matter classification. Part of [`SemanticsConfig`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrontMatterConfig {
    /// The title starts in the upper part of the page: its top must be above
    /// this fraction of the page height.
    pub title_max_top_ratio: f32,
    /// Fewest characters of a block that may set the title size (shorter
    /// ones are stray symbols or logos).
    pub title_min_chars_count: usize,
    /// The largest text must exceed the body size by at least this much
    /// (points) to be a title; otherwise the page has no title and no front
    /// matter is classified.
    pub title_min_size_above_body_pt: f32,
    /// Blocks within this distance (points) below the title size belong to
    /// the title (a title in two sizes, or with a subtitle).
    pub title_size_tolerance_pt: f32,
    /// Share of bold glyphs from which a numbered line counts as bold, as a
    /// section heading that ends the author block.
    pub section_bold_min_ratio: f32,
    /// A numbered line at least this much larger than the body text
    /// (points) is a section heading.
    pub section_min_size_above_body_pt: f32,
    /// A numbered line with a larger share of capitals is a section heading.
    pub section_min_upper_ratio: f32,
    /// Section headings are shorter than this (characters); longer numbered
    /// text is an affiliation or a footnote.
    pub section_max_chars_count: usize,
    /// Fewest characters of a line of running prose.
    pub prose_line_min_chars_count: usize,
    /// A prose line ends within this distance (points) of the block's right
    /// edge (justified text).
    pub prose_right_tolerance_pt: f32,
    /// A prose line starts within this distance (multiple of its font size)
    /// of the block's left edge (allowing for a paragraph indent).
    pub prose_left_tolerance_em: f32,
    /// Fewest lines of a block of running prose.
    pub prose_min_lines_count: usize,
    /// Fewest full (justified) lines of a block of running prose.
    pub prose_min_full_lines_count: usize,
    /// An abstract without a heading must end at most this many pages after
    /// the first page.
    pub abstract_max_page_offset_count: usize,
    /// The heading that ends an abstract without a heading is shorter than
    /// this (characters).
    pub abstract_heading_max_chars_count: usize,
    /// Most text blocks of an abstract without a heading; more prose before
    /// the first heading is the body itself.
    pub abstract_max_blocks_count: usize,
    /// Author blocks laid out beside the stop block must end at least this
    /// far (points) above it.
    pub author_stop_margin_pt: f32,
    /// Author blocks laid out beside the title column may start this far
    /// (points) above the bottom of the title.
    pub author_title_margin_pt: f32,
    /// Lines of the same author card are at most this far apart vertically
    /// (multiple of the larger font size); the clustering stops looking
    /// further down beyond it.
    pub card_max_gap_em: f32,
    /// Lines whose baselines differ by less than this (points) are on the
    /// same row and belong to different cards.
    pub card_same_row_tolerance_pt: f32,
    /// Lines of the same card overlap horizontally by at least this
    /// fraction of the narrower one.
    pub card_min_x_overlap_ratio: f32,
    /// Cards on the same row overlap vertically by at least this fraction
    /// of the lower one's height (or the row's).
    pub card_row_min_y_overlap_ratio: f32,
    /// A zone shared with the author grid is made single-column when it has
    /// at most this many other nodes (typically the "Abstract" heading).
    pub author_zone_max_others_count: usize,
}

impl Default for FrontMatterConfig {
    fn default() -> Self {
        Self {
            title_max_top_ratio: 0.65,
            title_min_chars_count: 4,
            title_min_size_above_body_pt: 1.4,
            title_size_tolerance_pt: 0.6,
            section_bold_min_ratio: 0.6,
            section_min_size_above_body_pt: 0.5,
            section_min_upper_ratio: 0.8,
            section_max_chars_count: 80,
            prose_line_min_chars_count: 40,
            prose_right_tolerance_pt: 1.5,
            prose_left_tolerance_em: 2.0,
            prose_min_lines_count: 4,
            prose_min_full_lines_count: 3,
            abstract_max_page_offset_count: 1,
            abstract_heading_max_chars_count: 80,
            abstract_max_blocks_count: 3,
            author_stop_margin_pt: 1.0,
            author_title_margin_pt: 2.0,
            card_max_gap_em: 0.9,
            card_same_row_tolerance_pt: 2.0,
            card_min_x_overlap_ratio: 0.5,
            card_row_min_y_overlap_ratio: 0.4,
            author_zone_max_others_count: 3,
        }
    }
}

/// One group of author lines (a name with its affiliation and e-mail),
/// placed as a unit.
struct AuthorCard {
    /// Bounding box of the lines.
    bbox: Rect,
    /// The lines, top to bottom.
    lines: Vec<Line>,
}

/// Where the author block ends.
struct AuthorEnd {
    /// Index of the first node after the author block (the stop node, or
    /// the first node of the next page).
    end: usize,
    /// The block was ended by running prose rather than by an abstract or a
    /// section heading.
    stopped_by_prose: bool,
}

/// Classifies the front matter of the first page with text: marks notes,
/// title and authors (replacing the author nodes by re-ordered cards), and
/// an abstract without a heading. Does nothing when no text is large enough
/// to be a title.
pub(crate) fn classify_front_matter(
    nodes: &mut Vec<Node>,
    pages: &[PageData],
    stats: &DocStats,
    fonts: &[FontInfo],
    cfg: &SemanticsConfig,
) {
    let front = &cfg.front_matter;
    let Some(first_page) = nodes
        .iter()
        .find(|node| node.text_block().is_some())
        .map(|node| node.loc.page)
    else {
        return;
    };
    let page_h = pages[first_page].height;
    let Some((title_start, title_end)) = find_title(nodes, first_page, page_h, stats, front) else {
        return;
    };
    for node in nodes[..title_start]
        .iter_mut()
        .filter(|node| node.loc.page == first_page)
    {
        if node.text_block().is_some() {
            node.class = Class::Note;
        }
    }
    for node in &mut nodes[title_start..=title_end] {
        node.class = Class::Title;
    }

    // Authors: everything up to the abstract or the first section.
    let author_end = find_author_end(nodes, title_end, first_page, stats, fonts, cfg);
    let end = author_end.end;
    // An abstract without a heading (journal layouts): the prose that ended
    // the author block, up to the first heading.
    if author_end.stopped_by_prose {
        mark_unheaded_abstract(nodes, end, first_page, front);
    }
    let author_idx = author_indices(nodes, (title_start, title_end), end, first_page, front);
    if author_idx.is_empty() {
        return;
    }
    let author_zones: HashSet<usize> = author_idx.iter().map(|&idx| nodes[idx].loc.zone).collect();
    replace_author_nodes(nodes, &author_idx, front);
    flatten_author_zones(nodes, &author_zones, first_page, front);
}

/// The range of node indices (inclusive) of the title: the consecutive
/// nodes on the first page, in its upper part, whose size is close to the
/// largest size found there. Returns `None` when the largest text is not
/// clearly larger than the body text, or is a known heading ("Contents").
fn find_title(
    nodes: &[Node],
    first_page: usize,
    page_h: f32,
    stats: &DocStats,
    cfg: &FrontMatterConfig,
) -> Option<(usize, usize)> {
    let on_first = |node: &&Node| node.loc.page == first_page;
    // Title: the largest text in the upper part of the first page.
    let mut title_size = 0.0f32;
    for node in nodes.iter().filter(on_first) {
        if let Some(block) = node.text_block()
            && block.bbox.y0 < cfg.title_max_top_ratio * page_h
            && block_text(&block.lines).chars().count() >= cfg.title_min_chars_count
        {
            title_size = title_size.max(block_size(&block.lines));
        }
    }
    let mut title_range: Option<(usize, usize)> = None;
    if title_size >= stats.body_size + cfg.title_min_size_above_body_pt {
        for (idx, node) in nodes.iter().enumerate() {
            if !on_first(&node) {
                continue;
            }
            let is_title = node.text_block().is_some_and(|block| {
                block_size(&block.lines) >= title_size - cfg.title_size_tolerance_pt
                    && block.bbox.y0 < cfg.title_max_top_ratio * page_h
            });
            match (&mut title_range, is_title) {
                (None, true) => title_range = Some((idx, idx)),
                (Some((_, range_end)), true) if *range_end + 1 == idx => *range_end = idx,
                _ => {}
            }
        }
    }
    // A heading such as "Contents" is not a title: the first page with text
    // is then not a title page (the title is on a cover that is a picture).
    title_range.filter(|&(start, end)| {
        let text: Vec<String> = nodes[start..=end]
            .iter()
            .filter_map(|node| node.text_block().map(|block| block_text(&block.lines)))
            .collect();
        !KNOWN_HEADINGS.is_match(text.join(" ").trim())
    })
}

/// Finds where the author block after the title ends: at the first text
/// block on the first page that starts the abstract, is a section heading,
/// or is running prose; at the end of the first page otherwise.
fn find_author_end(
    nodes: &[Node],
    title_end: usize,
    first_page: usize,
    stats: &DocStats,
    fonts: &[FontInfo],
    cfg: &SemanticsConfig,
) -> AuthorEnd {
    let mut end = title_end + 1;
    while end < nodes.len() && nodes[end].loc.page == first_page {
        if let Some(block) = nodes[end].text_block() {
            let text = block_text(&block.lines);
            let trimmed = text.trim();
            let is_abstract = is_abstract_start(trimmed);
            let is_section = is_section_start(block, trimmed, stats, fonts, cfg);
            let prose = is_running_prose(block, &cfg.front_matter);
            if is_abstract || is_section || prose {
                return AuthorEnd {
                    end,
                    stopped_by_prose: prose && !is_abstract && !is_section,
                };
            }
        }
        end += 1;
    }
    AuthorEnd {
        end,
        stopped_by_prose: false,
    }
}

/// "Abstract" as a heading, or an inline "Abstract—" label.
fn is_abstract_start(trimmed: &str) -> bool {
    trimmed.eq_ignore_ascii_case("abstract") || ABSTRACT_INLINE.is_match(trimmed)
}

/// A numbered section heading ("1 Introduction", "I. INTRODUCTION") or an
/// unnumbered "Introduction" that is bold, larger than the body text or in
/// capitals, and short.
fn is_section_start(
    block: &TextBlock,
    trimmed: &str,
    stats: &DocStats,
    fonts: &[FontInfo],
    cfg: &SemanticsConfig,
) -> bool {
    let front = &cfg.front_matter;
    let bold = frac(&block.lines, fonts, |font| font.bold) >= front.section_bold_min_ratio;
    (HEADING_NUM.is_match(trimmed) || HEADING_ROMAN.is_match(trimmed) || INTRO_HEADING.is_match(trimmed))
        && (bold
            || block_size(&block.lines) > stats.body_size + front.section_min_size_above_body_pt
            || upper_ratio(trimmed, cfg) > front.section_min_upper_ratio)
        && trimmed.chars().count() < front.section_max_chars_count
}

/// Running prose: several justified lines.
fn is_running_prose(block: &TextBlock, cfg: &FrontMatterConfig) -> bool {
    let full = block
        .lines
        .iter()
        .filter(|line| {
            line.char_count() >= cfg.prose_line_min_chars_count
                && (line.bbox.x1 - block.right).abs() < cfg.prose_right_tolerance_pt
                && (line.bbox.x0 - block.left).abs() < cfg.prose_left_tolerance_em * line.size
        })
        .count();
    block.lines.len() >= cfg.prose_min_lines_count && full >= cfg.prose_min_full_lines_count
}

/// Marks the text nodes from `start` up to the first heading as abstract
/// paragraphs, if that heading is found on the first pages and only a few
/// text blocks precede it. Leaves everything unchanged otherwise.
fn mark_unheaded_abstract(nodes: &mut [Node], start: usize, first_page: usize, cfg: &FrontMatterConfig) {
    let last_page = first_page + cfg.abstract_max_page_offset_count;
    let next_heading = nodes[start..].iter().position(|node| {
        node.loc.page > last_page
            || node.text_block().is_some_and(|block| {
                let text = block_text(&block.lines);
                let text = text.trim();
                text.chars().count() < cfg.abstract_heading_max_chars_count
                    && (HEADING_NUM.is_match(text) || HEADING_ROMAN.is_match(text) || KNOWN_HEADINGS.is_match(text))
            })
    });
    let Some(len) = next_heading else { return };
    let abstract_nodes = start..start + len;
    let texts = nodes[abstract_nodes.clone()]
        .iter()
        .filter(|node| node.text_block().is_some())
        .count();
    if nodes[start + len].loc.page <= last_page && texts <= cfg.abstract_max_blocks_count {
        for node in nodes[abstract_nodes]
            .iter_mut()
            .filter(|node| node.text_block().is_some())
        {
            node.class = Class::Paragraph { in_abstract: true };
        }
    }
}

/// Indices of the unclassified text nodes of the author block: those
/// between the title and `end`, plus those that lie geometrically between
/// the title and the stop block (the column analysis may have ordered a
/// second author column later).
fn author_indices(
    nodes: &[Node],
    (title_start, title_end): (usize, usize),
    end: usize,
    first_page: usize,
    cfg: &FrontMatterConfig,
) -> Vec<usize> {
    let title_bottom = nodes[title_start..=title_end]
        .iter()
        .filter_map(|node| node.text_block())
        .map(|block| block.bbox.y1)
        .fold(0.0, f32::max);
    let stop_y = nodes
        .get(end)
        .filter(|node| node.loc.page == first_page)
        .and_then(|node| node.text_block())
        .map(|block| block.bbox.y0)
        .unwrap_or(f32::INFINITY);
    (title_end + 1..nodes.len())
        .filter(|&idx| {
            let node = &nodes[idx];
            let Some(block) = node.text_block() else { return false };
            node.loc.page == first_page
                && node.class == Class::Unknown
                && (idx < end
                    || (stop_y.is_finite()
                        && block.bbox.y1 <= stop_y - cfg.author_stop_margin_pt
                        && block.bbox.y0 >= title_bottom - cfg.author_title_margin_pt))
        })
        .collect()
}

/// Replaces the author nodes at `author_idx` (sorted, non-empty) by one
/// [`Class::Authors`] node per card, ordered row by row, inserted where the
/// first author node was. Other nodes in between (floats etc.) are kept.
fn replace_author_nodes(nodes: &mut Vec<Node>, author_idx: &[usize], cfg: &FrontMatterConfig) {
    // Author blocks are laid out as a grid of small centred groups, which
    // the column analysis cannot order. Re-cluster their lines into one
    // "card" per group and order the cards row by row.
    let mut lines: Vec<Line> = Vec::new();
    for &idx in author_idx {
        lines.extend(nodes[idx].text_block().unwrap().lines.iter().cloned());
    }
    lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
    let ordered = order_cards(cluster_cards(&lines, cfg), cfg);

    let template = &nodes[author_idx[0]];
    let template_loc = Loc {
        zone_cols: 1,
        col: 0,
        zone: AUTHOR_ZONE,
        ..template.loc
    };
    let (leaf_left, leaf_right) = (template.leaf_left, template.leaf_right);
    let new_nodes: Vec<Node> = ordered
        .into_iter()
        .map(|card| Node {
            kind: NodeKind::Text(TextBlock {
                lines: card.lines,
                loc: template_loc,
                left: card.bbox.x0,
                right: card.bbox.x1,
                centered: true,
                gap_before: f32::INFINITY,
                bbox: card.bbox,
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
    let mut idx = 0;
    nodes.retain(|_| {
        let keep = !drop.contains(&idx);
        idx += 1;
        keep
    });
    nodes.splice(first..first, new_nodes);
}

/// Groups author lines (sorted by baseline, then left edge) into cards:
/// lines on different rows that overlap horizontally and are close
/// vertically belong together (transitively). Cards are returned in the
/// order of their first line.
fn cluster_cards(lines: &[Line], cfg: &FrontMatterConfig) -> Vec<AuthorCard> {
    let mut parent: Vec<usize> = (0..lines.len()).collect();
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            let (upper, lower) = (&lines[i], &lines[j]);
            let gap = lower.bbox.y0 - upper.bbox.y1;
            if gap > cfg.card_max_gap_em * upper.size.max(lower.size) {
                break;
            }
            let same_row = (upper.baseline - lower.baseline).abs() < cfg.card_same_row_tolerance_pt;
            if !same_row
                && upper.bbox.x_overlap(&lower.bbox)
                    >= cfg.card_min_x_overlap_ratio * upper.bbox.width().min(lower.bbox.width())
            {
                let (root_a, root_b) = (find_root(&mut parent, i), find_root(&mut parent, j));
                if root_a != root_b {
                    parent[root_b] = root_a;
                }
            }
        }
    }
    let mut cards: Vec<AuthorCard> = Vec::new();
    let mut card_of: HashMap<usize, usize> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let root = find_root(&mut parent, i);
        let card_idx = *card_of.entry(root).or_insert_with(|| {
            cards.push(AuthorCard {
                bbox: Rect::EMPTY,
                lines: Vec::new(),
            });
            cards.len() - 1
        });
        cards[card_idx].bbox = cards[card_idx].bbox.union(&line.bbox);
        cards[card_idx].lines.push(line.clone());
    }
    cards
}

/// Root of `i` in the union-find forest `parent`; points `i` directly at
/// the root on the way.
fn find_root(parent: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while parent[root] != root {
        root = parent[root];
    }
    parent[i] = root;
    root
}

/// Orders cards in reading order: rows of vertically overlapping cards, top
/// to bottom, and left to right within a row.
fn order_cards(mut cards: Vec<AuthorCard>, cfg: &FrontMatterConfig) -> Vec<AuthorCard> {
    // Rows of cards.
    cards.sort_by(|a, b| a.bbox.y0.total_cmp(&b.bbox.y0));
    let mut ordered: Vec<AuthorCard> = Vec::new();
    let mut row: Vec<AuthorCard> = Vec::new();
    let mut row_box = Rect::EMPTY;
    for card in cards {
        if !row.is_empty()
            && row_box.y_overlap(&card.bbox)
                < cfg.card_row_min_y_overlap_ratio * row_box.height().min(card.bbox.height())
        {
            row.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
            ordered.append(&mut row);
            row_box = Rect::EMPTY;
        }
        row_box = row_box.union(&card.bbox);
        row.push(card);
    }
    row.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
    ordered.append(&mut row);
    ordered
}

/// Makes the few other nodes that shared a layout zone with the author
/// grid (typically the "Abstract" heading) single-column: they are not
/// really part of a multi-column zone.
fn flatten_author_zones(nodes: &mut [Node], author_zones: &HashSet<usize>, first_page: usize, cfg: &FrontMatterConfig) {
    for &zone in author_zones {
        let others: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.loc.page == first_page && node.loc.zone == zone && node.class != Class::Authors)
            .map(|(idx, _)| idx)
            .collect();
        if others.len() <= cfg.author_zone_max_others_count {
            for idx in others {
                nodes[idx].loc.zone_cols = 1;
                nodes[idx].loc.col = 0;
            }
        }
    }
}
