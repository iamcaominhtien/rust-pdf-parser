//! Pictures cut out of the page must not cut through text.
//!
//! Equations are shown as pictures of their region of the page. The region
//! is the ink of the equation's lines plus a margin, so it can cut glyphs
//! that belong to something else: a piece of the same display that was put
//! in another block (a numerator, the limits of a sum, the other half of a
//! line split by a wrong column), or the descenders of the paragraph above.
//! The pictures then show halves of letters.
//!
//! [`fit_equation_crops`] makes the regions clean: an equation whose region
//! holds glyphs of another equation absorbs it (they are one display), and
//! a region (of an equation or a figure) is trimmed where it reaches into
//! the text around it: the first letters of the next column, a running
//! head, the top of the paragraph below.
//! [`cut_glyphs`] measures what is left, for every picture (equations and
//! figures), and is what the tests check.

use crate::common::geom::Rect;
use crate::model::{Block, BlockKind, Document, Line, PLine, Span};

/// Overlap, in points, below which a glyph only touches a region.
const TOUCH_PT: f32 = 0.6;
/// Space kept between a trimmed region and the text it was trimmed against,
/// in points.
const TRIM_GAP_PT: f32 = 0.3;
/// A figure region is trimmed away from text that reaches at most this far
/// into it, in points (about a line): deeper, the text may be part of the
/// drawing and the region is left alone.
const FIGURE_TRIM_MAX_PT: f32 = 8.0;

/// A picture that cuts glyphs.
#[derive(Debug, Clone)]
pub struct Cut {
    /// Zero-based page index.
    pub page: usize,
    /// Index of the block in the document.
    pub block: usize,
    /// "equation" or "figure".
    pub kind: &'static str,
    /// Number of glyphs the picture cuts.
    pub glyphs: usize,
    /// The region of the picture.
    pub rect: Rect,
    /// A few of the cut glyphs, as "text@x,y" with the kind of block they
    /// belong to.
    pub examples: Vec<String>,
}

/// Whether `ink` is cut by `rect`: they overlap by more than a touch in
/// both directions, and `ink` is not inside `rect`.
fn cuts(rect: &Rect, ink: &Rect) -> bool {
    let overlap = rect.intersect(ink);
    !overlap.is_unset()
        && overlap.width() > TOUCH_PT
        && overlap.height() > TOUCH_PT
        && !rect.inflate(TOUCH_PT, TOUCH_PT).contains(ink)
}

/// The ink of the visible glyphs of some lines.
fn inks(lines: &[Line]) -> impl Iterator<Item = Rect> + '_ {
    lines
        .iter()
        .flat_map(|line| line.glyphs.iter())
        .filter(|glyph| !glyph.text.trim().is_empty() && !glyph.ink.is_unset())
        .map(|glyph| glyph.ink)
}

/// The glyph inks of a page, each with the index of the block it belongs
/// to (`None` for loose lines: running heads, page numbers).
fn page_glyphs(doc: &Document, loose: &[Vec<PLine>], page: usize) -> Vec<(Rect, Option<usize>)> {
    page_glyph_texts(doc, loose, page)
        .into_iter()
        .map(|(ink, owner, _)| (ink, owner))
        .collect()
}

/// [`page_glyphs`] with the text of each glyph.
fn page_glyph_texts(doc: &Document, loose: &[Vec<PLine>], page: usize) -> Vec<(Rect, Option<usize>, String)> {
    let visible = |line: &Line| {
        line.glyphs
            .iter()
            .filter(|glyph| !glyph.text.trim().is_empty() && !glyph.ink.is_unset())
            .map(|glyph| (glyph.ink, glyph.text.clone()))
            .collect::<Vec<_>>()
    };
    let mut out = Vec::new();
    // A paragraph that runs on to the next page has lines on both: each
    // line knows its page.
    for (idx, block) in doc.blocks.iter().enumerate() {
        for line in block
            .lines
            .iter()
            .chain(&block.inner_lines)
            .filter(|line| line.page == page)
        {
            out.extend(visible(line).into_iter().map(|(ink, text)| (ink, Some(idx), text)));
        }
    }
    if let Some(lines) = loose.get(page) {
        for pline in lines.iter().filter(|pline| pline.kind != "eqno") {
            out.extend(visible(&pline.line).into_iter().map(|(ink, text)| (ink, None, text)));
        }
    }
    out
}

/// A short name for the kind of a block (in reports).
fn block_kind_name(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Equation { .. } => "equation",
        BlockKind::Figure { .. } => "figure",
        BlockKind::Paragraph { .. } => "paragraph",
        BlockKind::Heading { .. } => "heading",
        BlockKind::Footnote { .. } => "footnote",
        _ => "other",
    }
}

/// The region and kind of the picture of a block, if it has one.
fn picture(block: &Block) -> Option<(Rect, &'static str)> {
    match &block.kind {
        BlockKind::Equation { image, .. } => Some((image.rect, "equation")),
        BlockKind::Figure { image: Some(image), .. } if block.table.is_none() => Some((image.rect, "figure")),
        _ => None,
    }
}

/// The pictures that cut glyphs of another block (or of the page's loose
/// text), with how many. Glyphs of the block itself (labels inside a
/// figure) are not counted: they are drawn in the picture.
pub fn cut_glyphs(doc: &Document, loose: &[Vec<PLine>]) -> Vec<Cut> {
    let mut out = Vec::new();
    for page in 0..doc.pages.len() {
        let glyphs = page_glyph_texts(doc, loose, page);
        for (idx, block) in doc.blocks.iter().enumerate().filter(|(_, block)| block.page == page) {
            let Some((rect, kind)) = picture(block) else { continue };
            let cut: Vec<&(Rect, Option<usize>, String)> = glyphs
                .iter()
                .filter(|(ink, owner, _)| *owner != Some(idx) && cuts(&rect, ink))
                .collect();
            if !cut.is_empty() {
                let examples = cut
                    .iter()
                    .take(6)
                    .map(|(ink, owner, text)| {
                        let of = owner.map_or("loose", |other| block_kind_name(&doc.blocks[other].kind));
                        format!("{text}@{:.0},{:.0}({of})", ink.x0, ink.y0)
                    })
                    .collect();
                out.push(Cut {
                    page,
                    block: idx,
                    kind,
                    glyphs: cut.len(),
                    rect,
                    examples,
                });
            }
        }
    }
    out
}

/// The ink of an equation block (union of its glyphs).
fn own_ink(block: &Block) -> Rect {
    inks(&block.lines).fold(Rect::EMPTY, |acc, ink| acc.union(&ink))
}

/// Sets the picture region (and box) of an equation block.
fn set_region(block: &mut Block, rect: Rect) {
    if let BlockKind::Equation { image, .. } = &mut block.kind {
        image.rect = rect;
    }
    block.bbox = rect;
}

/// Makes the pictures of equations clean (see the module documentation):
/// equations whose regions hold each other's glyphs become one, then each
/// region is trimmed away from the text around it. Block ids are numbered
/// again afterwards.
pub(crate) fn fit_equation_crops(doc: &mut Document, loose: &[Vec<PLine>]) {
    let mut merged = false;
    for page in 0..doc.pages.len() {
        while let Some((keep, absorb)) = overlapping_equations(doc, loose, page) {
            merge_equations(doc, keep, absorb);
            merged = true;
        }
        trim_regions(doc, loose, page);
    }
    if merged {
        for (id, block) in doc.blocks.iter_mut().enumerate() {
            block.id = id;
        }
        single_column_zones(doc);
    }
}

/// A zone whose later columns lost all their blocks (the other halves of
/// equation lines a wrong column split had cut in two) is one column.
fn single_column_zones(doc: &mut Document) {
    let mut used: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for block in &doc.blocks {
        if block.loc.zone_cols > 1 && block.loc.col > 0 {
            used.insert((block.loc.page, block.loc.zone));
        }
    }
    for block in doc.blocks.iter_mut() {
        if block.loc.zone_cols > 1 && !used.contains(&(block.loc.page, block.loc.zone)) {
            block.loc.zone_cols = 1;
            block.loc.col = 0;
        }
    }
}

/// Two equations of the page where the region of one cuts or holds glyphs
/// of the other: (the earlier, the later) in reading order.
fn overlapping_equations(doc: &Document, loose: &[Vec<PLine>], page: usize) -> Option<(usize, usize)> {
    let glyphs = page_glyphs(doc, loose, page);
    let is_eq = |idx: usize| matches!(doc.blocks[idx].kind, BlockKind::Equation { .. });
    for (idx, block) in doc.blocks.iter().enumerate().filter(|(_, block)| block.page == page) {
        let BlockKind::Equation { image, .. } = &block.kind else {
            continue;
        };
        let own = own_ink(block);
        for (ink, owner) in &glyphs {
            let Some(other) = *owner else { continue };
            if other == idx || !is_eq(other) {
                continue;
            }
            // Held or cut, beyond a touch of the margin.
            let overlap = image.rect.intersect(ink);
            let reaches = !overlap.is_unset() && overlap.width() > TOUCH_PT && overlap.height() > TOUCH_PT;
            // A glyph that only sits in the margin of a region it does not
            // overlap the ink of is trimmed away instead (two equations one
            // above the other).
            let into_ink = !own.intersect(ink).is_unset() || cuts(&image.rect, ink);
            if reaches && into_ink {
                return Some((idx.min(other), idx.max(other)));
            }
        }
    }
    None
}

/// Moves the equation `absorb` into the equation `keep` (an earlier block).
fn merge_equations(doc: &mut Document, keep: usize, absorb: usize) {
    let gone = doc.blocks.remove(absorb);
    let block = &mut doc.blocks[keep];
    let (gone_rect, gone_number) = match gone.kind {
        BlockKind::Equation { image, number } => (image.rect, number),
        _ => return,
    };
    let rect = block.bbox.union(&gone_rect);
    if let BlockKind::Equation { number, .. } = &mut block.kind
        && number.is_none()
        && gone_number.is_some()
    {
        *number = gone_number;
        if gone.anchor.is_some() {
            block.anchor = gone.anchor;
        }
    }
    block.lines.extend(gone.lines);
    block
        .lines
        .sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
    block.line_texts = block.lines.iter().map(|line| line.text()).collect();
    block.spans = vec![Span::plain(block.line_texts.join(" "))];
    block.eq_runs.extend(gone.eq_runs);
    set_region(block, rect);
}

/// Trims the equation regions of a page away from glyphs of other blocks
/// that only reach into their margin.
fn trim_regions(doc: &mut Document, loose: &[Vec<PLine>], page: usize) {
    let glyphs = page_glyphs(doc, loose, page);
    for idx in 0..doc.blocks.len() {
        let block = &doc.blocks[idx];
        if block.page != page {
            continue;
        }
        if matches!(block.kind, BlockKind::Figure { .. }) {
            trim_figure(&mut doc.blocks[idx], idx, &glyphs);
            continue;
        }
        let BlockKind::Equation { image, .. } = &block.kind else {
            continue;
        };
        let own = own_ink(block);
        if own.is_unset() {
            continue;
        }
        let mut rect = image.rect.union(&own);
        for (ink, owner) in &glyphs {
            if *owner == Some(idx) || !cuts(&rect, ink) {
                continue;
            }
            if ink.y1 <= own.y0 {
                rect.y0 = rect.y0.max(ink.y1 + TRIM_GAP_PT).min(own.y0);
            } else if ink.y0 >= own.y1 {
                rect.y1 = rect.y1.min(ink.y0 - TRIM_GAP_PT).max(own.y1);
            } else if ink.x1 <= own.x0 {
                rect.x0 = rect.x0.max(ink.x1 + TRIM_GAP_PT).min(own.x0);
            } else if ink.x0 >= own.x1 {
                rect.x1 = rect.x1.min(ink.x0 - TRIM_GAP_PT).max(own.x1);
            }
        }
        set_region(&mut doc.blocks[idx], rect);
    }
}

/// Trims the region of a figure away from glyphs of other blocks that
/// reach into it from an edge by at most [`FIGURE_TRIM_MAX_PT`]: each such
/// glyph moves the edge it is nearest to.
fn trim_figure(block: &mut Block, idx: usize, glyphs: &[(Rect, Option<usize>)]) {
    let BlockKind::Figure { image: Some(image), .. } = &mut block.kind else {
        return;
    };
    if block.table.is_some() {
        return;
    }
    let mut rect = image.rect;
    for (ink, owner) in glyphs {
        if *owner == Some(idx) || !cuts(&rect, ink) {
            continue;
        }
        // How far the glyph reaches in from each edge.
        let depths = [
            ink.y1 - rect.y0, // from the top
            rect.y1 - ink.y0, // from the bottom
            ink.x1 - rect.x0, // from the left
            rect.x1 - ink.x0, // from the right
        ];
        let (edge, depth) = depths
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map(|(edge, depth)| (edge, *depth))
            .unwrap();
        if depth > FIGURE_TRIM_MAX_PT {
            continue;
        }
        match edge {
            0 => rect.y0 = ink.y1 + TRIM_GAP_PT,
            1 => rect.y1 = ink.y0 - TRIM_GAP_PT,
            2 => rect.x0 = ink.x1 + TRIM_GAP_PT,
            _ => rect.x1 = ink.x0 - TRIM_GAP_PT,
        }
    }
    if rect.width() > 1.0 && rect.height() > 1.0 {
        image.rect = rect;
    }
}
