//! The analysis pipeline: runs every stage, from raw pages to the [`Document`].
//!
//! [`analyze`] reads from top to bottom as the whole flow: page data and
//! document statistics, furniture removal, float detection, reading order
//! and blocks per page, then the document-wide structuring stages of
//! [`semantics`](super::semantics) and the assembly of the [`Document`].

use super::semantics::*;
use crate::analysis::blocks::{RawBlock, build_blocks};
use crate::analysis::floats::{Float, FloatsConfig, detect_floats, join_caption_labels, table_caption_votes};
use crate::analysis::furniture::remove_furniture;
use crate::analysis::lines::join_math_letters;
use crate::analysis::order::order_page;
use crate::analysis::page::{DocStats, PageData, compute_stats};
use crate::backend::{FontInfo, RawPage};
use crate::config::AnalyzeOptions;
use crate::model::*;

/// Table captions are taken to sit above (or below) their tables when the
/// votes for that side outnumber the other side by more than this factor;
/// otherwise each table is decided on its own. This is a tuning value of
/// float detection: it belongs in `FloatsConfig`, which is defined outside
/// this module, so it is kept here as a constant for now.
const TABLE_CAPTION_MAJORITY_FACTOR: usize = 2;

/// Result of the analysis of a paper.
pub struct Analysis {
    /// The structured document.
    pub doc: Document,
    /// Lines that belong to no block (running heads, page numbers, equation
    /// numbers), per page. The position-preserving renderers draw them.
    pub loose: Vec<Vec<PLine>>,
    /// Rotated text, per page.
    pub rotated: Vec<Vec<RotatedRun>>,
}

/// Runs the whole analysis on the extracted pages of a PDF.
///
/// `fonts` is the font table the glyphs refer to, `meta` the metadata found
/// so far (completed with the arXiv identifier and stamp found in the
/// margins, and with the title). `opts` selects table reconstruction and
/// holds every threshold.
pub fn analyze(raw_pages: Vec<RawPage>, fonts: &[FontInfo], mut meta: Meta, opts: AnalyzeOptions) -> Analysis {
    let layout = &opts.layout;
    let mut pages: Vec<PageData> = raw_pages
        .into_iter()
        .map(|raw| PageData::from_raw(raw, &layout.lines))
        .collect();
    for page in &mut pages {
        join_math_letters(&mut page.lines, fonts, &layout.lines);
    }
    let stats = compute_stats(&pages, &layout.page);
    let furniture = remove_furniture(&mut pages, &layout.furniture);
    meta.arxiv_id = meta.arxiv_id.or(furniture.arxiv_id);
    meta.arxiv_stamp = furniture.arxiv_stamp.clone();

    // --- per-page layout ---
    let mut nodes: Vec<Node> = Vec::new();
    for page in pages.iter_mut() {
        join_caption_labels(page, &layout.floats, &layout.lines);
    }
    let table_caption_above = table_caption_convention(&pages, &stats, fonts, &layout.floats);
    for page in pages.iter_mut() {
        layout_page(page, &stats, fonts, table_caption_above, &opts, &mut nodes);
    }

    // --- document structure ---
    classify_front_matter(&mut nodes, &pages, &stats, fonts, &layout.semantics);
    classify_body(&mut nodes, &stats, fonts, &layout.semantics);
    split_references(&mut nodes, &layout.semantics);
    merge_paragraphs(&mut nodes, &stats, fonts, &layout.semantics);

    let rotated = pages.iter().map(|page| rotated_runs(page, fonts)).collect();
    let (mut doc, loose) = build_document(nodes, &pages, &stats, fonts, meta, furniture.lines, opts);
    // Pictures of equations and figures that cut glyphs are made clean.
    crate::analysis::crops::fit_equation_crops(&mut doc, &loose);
    // Displayed formulas get their structure, for MathML.
    crate::analysis::math::build_math(&mut doc, &pages, fonts, &layout.math);
    Analysis { doc, loose, rotated }
}

/// Convention for table captions in this document: `Some(true)` if they
/// clearly sit above their tables, `Some(false)` if clearly below, `None`
/// when the pages do not agree.
fn table_caption_convention(
    pages: &[PageData],
    stats: &DocStats,
    fonts: &[FontInfo],
    cfg: &FloatsConfig,
) -> Option<bool> {
    let (mut votes_above, mut votes_below) = (0, 0);
    for page in pages {
        let (above, below) = table_caption_votes(page, stats, fonts, cfg);
        votes_above += above;
        votes_below += below;
    }
    if votes_above > TABLE_CAPTION_MAJORITY_FACTOR * votes_below {
        Some(true)
    } else if votes_below > TABLE_CAPTION_MAJORITY_FACTOR * votes_above {
        Some(false)
    } else {
        None
    }
}

/// Lays out one page (floats, reading order, blocks) and appends a node for
/// each of its blocks to `nodes`, in reading order.
fn layout_page(
    page: &mut PageData,
    stats: &DocStats,
    fonts: &[FontInfo],
    table_caption_above: Option<bool>,
    opts: &AnalyzeOptions,
    nodes: &mut Vec<Node>,
) {
    let layout = &opts.layout;
    let floats = detect_floats(page, stats, fonts, table_caption_above, &layout.floats);

    let leaves = order_page(&page.lines, &floats, stats, &layout.order);
    let leaf_bounds: Vec<(f32, f32)> = leaves.iter().map(|leaf| (leaf.bbox.x0, leaf.bbox.x1)).collect();
    for raw_block in build_blocks(page, &leaves, stats, fonts, &layout.blocks, &layout.lines) {
        nodes.push(new_node(raw_block, &floats, &leaf_bounds));
    }
}

/// An unclassified node for a raw block. `floats` are the floats of the
/// block's page (a float block refers to one by index) and `leaf_bounds`
/// the left and right edges of the page's layout leaves.
fn new_node(raw_block: RawBlock, floats: &[Float], leaf_bounds: &[(f32, f32)]) -> Node {
    let (kind, loc) = match raw_block {
        RawBlock::Text(block) => {
            let loc = block.loc;
            (NodeKind::Text(block), loc)
        }
        RawBlock::Footnote(block) => {
            let loc = block.loc;
            (NodeKind::Footnote(block), loc)
        }
        RawBlock::Equation(equation) => {
            let loc = equation.loc;
            (NodeKind::Equation(equation), loc)
        }
        RawBlock::Float { idx, loc } => (NodeKind::Float(floats[idx].clone()), loc),
    };
    let (leaf_left, leaf_right) = leaf_bounds[loc.leaf];
    Node {
        kind,
        loc,
        class: Class::Unknown,
        tail: (loc.page, loc.leaf),
        leaf_left,
        leaf_right,
        strip_prefix: 0,
    }
}

/// The rotated text of a page as output runs, without runs that are blank
/// or contain glyphs without a Unicode value.
fn rotated_runs(page: &PageData, fonts: &[FontInfo]) -> Vec<RotatedRun> {
    page.rotated
        .iter()
        .filter(|run| !run.text.contains('\u{FFFD}') && !run.text.trim().is_empty())
        .map(|run| RotatedRun {
            text: run.text.clone(),
            x: run.x,
            y: run.y,
            angle: run.angle,
            size: run.size,
            length: run.length,
            style: font_style(&fonts[run.font as usize]),
            color: run.color,
            origins: run.origins.clone(),
        })
        .collect()
}

/// Style bits (`BOLD`, `ITALIC`, `MONO`, `SANS`) of a font.
fn font_style(font: &FontInfo) -> u8 {
    let mut style = 0;
    if font.bold {
        style |= BOLD;
    }
    if font.italic {
        style |= ITALIC;
    }
    if font.mono {
        style |= MONO;
    }
    if font.sans {
        style |= SANS;
    }
    if font.small_caps {
        style |= SMALL_CAPS;
    }
    style
}
