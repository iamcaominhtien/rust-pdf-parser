//! Medium mode: the page and column structure of the paper is kept, but the
//! text is real, reflowed HTML and paragraphs are never cut at column or page
//! boundaries.

use super::*;
use crate::common::geom::median;

/// Style sheet of the mode, embedded in the page.
const CSS: &str = include_str!("medium.css");
/// Smooth scrolling for links followed on the page, turned on once the page
/// has opened: a link from elsewhere (`#sec-3`) jumps straight to its place
/// instead of gliding over the whole paper.
const SMOOTH_JS: &str = "addEventListener('load',function(){requestAnimationFrame(function(){\
requestAnimationFrame(function(){document.documentElement.setAttribute('data-smooth','')})})})";

/// A table wider than its page column gets a smaller font (down to half),
/// relative to the page text so it scales with the page; what is still too
/// wide scrolls inside its frame.
const FIT_TABLES_JS: &str = "(function(){function fit(){[].forEach.call(document.querySelectorAll('.tbl-wrap>table'),\
function(t){t.style.fontSize='';var w=t.parentNode;if(!w.clientWidth)return;\
var rel=parseFloat(getComputedStyle(t).fontSize)/parseFloat(getComputedStyle(w).fontSize),k=1;\
for(var i=0;i<4&&t.scrollWidth>w.clientWidth+1;i++){var n=Math.max(0.5,k*w.clientWidth/t.scrollWidth);\
if(n>=k-0.005)break;k=n;t.style.fontSize=(rel*k*100).toFixed(2)+'%'}})}\
fit();if(document.fonts&&document.fonts.ready)document.fonts.ready.then(fit);addEventListener('resize',fit)})()";

/// Page margin used when no page has enough blocks to measure it, in points
/// (a little under an inch, typical of papers).
const DEFAULT_MARGIN_PT: f32 = 60.0;
/// Smallest and largest horizontal page margin, in points: keeps the text
/// readable when the measured extent is off (full-bleed figures, wide
/// tables).
const MARGIN_X_RANGE_PT: (f32, f32) = (18.0, 130.0);
/// Smallest and largest top page margin, in points.
const MARGIN_Y_RANGE_PT: (f32, f32) = (24.0, 110.0);
/// Largest margin shown around the text, in points: a page keeps the text
/// width and type size of the PDF, but wide paper margins (1.5 inch in
/// some templates) are trimmed to this, so the page is narrower instead of
/// mostly empty.
const SHOWN_MARGIN_X_PT: f32 = 40.0;
/// Largest top and bottom margin shown, in points.
const SHOWN_MARGIN_Y_PT: f32 = 44.0;
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
    let trim = PageTrim {
        x: (mx - SHOWN_MARGIN_X_PT).max(0.0),
        y: (my - SHOWN_MARGIN_Y_PT).max(0.0),
    };
    let (mx, my) = (mx - trim.x, my - trim.y);
    let line_height = (doc.leading / doc.body_size).clamp(LINE_HEIGHT_RANGE.0, LINE_HEIGHT_RANGE.1);

    let mut pages = String::new();
    for (page_idx, page) in doc.pages.iter().enumerate() {
        pages.push_str(&page_html(doc, page_idx, page, &trim, images));
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
<script>{SMOOTH_JS}</script>
<script>{FIT_TABLES_JS}</script>
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

/// How much of the paper margins is cut off each side of every page, in
/// points.
struct PageTrim {
    /// Cut off the left and the right.
    x: f32,
    /// Cut off the top and the bottom.
    y: f32,
}

/// One page: its zones of text, then its footnotes, then its number. The
/// page is drawn `trim` narrower and shorter than the PDF page.
fn page_html(doc: &Document, page_idx: usize, page: &PageInfo, trim: &PageTrim, images: &mut ImageStore<'_>) -> String {
    let blocks: Vec<&Block> = doc.blocks.iter().filter(|block| block.page == page_idx).collect();
    let (width, height) = (
        (page.width - 2.0 * trim.x).max(1.0),
        (page.height - 2.0 * trim.y).max(1.0),
    );
    let ctx = FlowCtx {
        unit: WidthUnit::Cqw { page_width: width },
        offprint: false,
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
        w = width,
        h = height,
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
