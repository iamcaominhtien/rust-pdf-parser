//! Medium mode: the page and column structure of the paper is kept, but the
//! text is real, reflowed HTML and paragraphs are never cut at column or page
//! boundaries.

use super::*;
use crate::common::geom::median;

/// Style sheet of the mode, embedded in the page.
const CSS: &str = include_str!("medium.css");

/// Page margin used when no page has enough blocks to measure it, in points
/// (a little under an inch, typical of papers).
const DEFAULT_MARGIN_PT: f32 = 60.0;
/// Smallest and largest horizontal page margin, in points: keeps the text
/// readable when the measured extent is off (full-bleed figures, wide
/// tables).
const MARGIN_X_RANGE_PT: (f32, f32) = (18.0, 130.0);
/// Smallest and largest top page margin, in points.
const MARGIN_Y_RANGE_PT: (f32, f32) = (24.0, 110.0);
/// Smallest and largest line height, as a multiple of the font size: the
/// leading of the PDF, kept in a range that reads well as HTML.
const LINE_HEIGHT_RANGE: (f32, f32) = (1.22, 1.5);
/// Fewest blocks a page must have for its content extent to count as an
/// estimate of the margins (title pages and float pages are misleading).
const MARGIN_MIN_BLOCKS: usize = 2;

/// The `medium` mode: page and column layout kept, text reflowed.
pub(crate) struct Medium;

impl Renderer for Medium {
    fn render(&self, analysis: &Analysis, images: &mut ImageStore<'_>) -> String {
        render(&analysis.doc, images)
    }
}

/// Writes the whole document in medium mode. Pictures are requested from
/// `images` page by page, in reading order.
pub(crate) fn render(doc: &Document, images: &mut ImageStore<'_>) -> String {
    let title = doc_title(doc);
    let (mx, my) = page_margins(doc);
    let line_height = (doc.leading / doc.body_size).clamp(LINE_HEIGHT_RANGE.0, LINE_HEIGHT_RANGE.1);

    let mut pages = String::new();
    for (page_idx, page) in doc.pages.iter().enumerate() {
        pages.push_str(&page_html(doc, page_idx, page, images));
    }

    let source = arxiv_abs_link(doc);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="paper2html (medium)">
<title>{title}</title>
<style>
:root{{--fs:{fs};--lh:{lh:.3};--mx:{mx:.1};--my:{my:.1}}}
{CSS}</style>
</head>
<body class="medium">
<div class="topbar"><span class="t">{title}</span>{source}<span>{n} pages</span></div>
<main class="pages">
{pages}</main>
</body>
</html>
"#,
        title = esc(&title),
        fs = doc.body_size,
        lh = line_height,
        n = doc.pages.len(),
    )
}

/// Horizontal and top page margins in points, estimated from the extent of
/// the content: the median over the pages with enough blocks, clamped.
fn page_margins(doc: &Document) -> (f32, f32) {
    let mut lefts = Vec::new();
    let mut tops = Vec::new();
    for (page_idx, page) in doc.pages.iter().enumerate() {
        let on_page: Vec<&Block> = doc.blocks.iter().filter(|block| block.page == page_idx).collect();
        if on_page.len() < MARGIN_MIN_BLOCKS {
            continue;
        }
        let x0 = on_page.iter().map(|block| block.bbox.x0).fold(f32::INFINITY, f32::min);
        let x1 = on_page
            .iter()
            .map(|block| block.bbox.x1)
            .fold(f32::NEG_INFINITY, f32::max);
        let y0 = on_page.iter().map(|block| block.bbox.y0).fold(f32::INFINITY, f32::min);
        lefts.push(x0.min(page.width - x1));
        tops.push(y0);
    }
    let mx = median(&lefts, DEFAULT_MARGIN_PT).clamp(MARGIN_X_RANGE_PT.0, MARGIN_X_RANGE_PT.1);
    let my = median(&tops, DEFAULT_MARGIN_PT).clamp(MARGIN_Y_RANGE_PT.0, MARGIN_Y_RANGE_PT.1);
    (mx, my)
}

/// One page: its zones of text, then its footnotes, then its number.
fn page_html(doc: &Document, page_idx: usize, page: &PageInfo, images: &mut ImageStore<'_>) -> String {
    let blocks: Vec<&Block> = doc.blocks.iter().filter(|block| block.page == page_idx).collect();
    let ctx = FlowCtx {
        unit: WidthUnit::Cqw { page_width: page.width },
    };
    let mut inner = String::new();

    let (footnotes, flow): (Vec<&Block>, Vec<&Block>) = blocks
        .iter()
        .copied()
        .partition(|block| matches!(block.kind, BlockKind::Footnote { .. }));

    // Consecutive blocks of the same zone form one container.
    let mut i = 0;
    while i < flow.len() {
        let key = zone_key(flow[i]);
        let mut j = i;
        while j < flow.len() && zone_key(flow[j]) == key {
            j += 1;
        }
        zone_html(&flow[i..j], key.1.max(1), images, &ctx, &mut inner);
        i = j;
    }
    if !footnotes.is_empty() {
        inner.push_str("<div class=\"footnotes\">\n");
        inner.push_str(&render_flow(&footnotes, images, &ctx));
        inner.push_str("</div>\n");
    }
    format!(
        "<section class=\"page\" id=\"page-{n}\" data-page=\"{n}\" style=\"--pw:{w};--ph:{h}\">\n<div class=\"page-inner\">\n{inner}</div>\n<div class=\"page-no\">{n}</div>\n</section>\n",
        n = page_idx + 1,
        w = page.width,
        h = page.height,
    )
}

/// The zone a block is in, as (zone number + 1, column count). Adjacent
/// single-column zones form one flow, so they all get the key (0, 1).
fn zone_key(block: &Block) -> (usize, usize) {
    if block.loc.zone_cols <= 1 {
        (0, 1)
    } else {
        (block.loc.zone + 1, block.loc.zone_cols)
    }
}

/// Writes the blocks of one zone as a container with `cols` columns. Runs
/// of title and author blocks get their own markup, everything else flows.
fn zone_html(zone: &[&Block], cols: usize, images: &mut ImageStore<'_>, ctx: &FlowCtx, inner: &mut String) {
    if cols > 1 {
        inner.push_str(&format!("<div class=\"zone cols\" style=\"column-count:{cols}\">\n"));
    } else {
        inner.push_str("<div class=\"zone\">\n");
    }
    // Title and author runs get their own markup.
    let mut k = 0;
    while k < zone.len() {
        let start = k;
        match zone[k].kind {
            BlockKind::Title => {
                while k < zone.len() && matches!(zone[k].kind, BlockKind::Title) {
                    k += 1;
                }
                let parts: Vec<String> = zone[start..k].iter().map(|block| spans_html(&block.spans)).collect();
                inner.push_str(&format!("<h1 class=\"title\">{}</h1>\n", parts.join(" ")));
            }
            BlockKind::Authors => {
                while k < zone.len() && matches!(zone[k].kind, BlockKind::Authors) {
                    k += 1;
                }
                inner.push_str(&authors_html(&zone[start..k]));
            }
            _ => {
                while k < zone.len() && !matches!(zone[k].kind, BlockKind::Title | BlockKind::Authors) {
                    k += 1;
                }
                inner.push_str(&render_flow(&zone[start..k], images, ctx));
            }
        }
    }
    inner.push_str("</div>\n");
}
