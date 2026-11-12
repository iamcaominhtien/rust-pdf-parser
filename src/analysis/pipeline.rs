//! The analysis pipeline: runs every stage, from raw pages to the [`Document`].

use super::semantics::*;
use crate::analysis::blocks::{RawBlock, build_blocks};
use crate::analysis::floats::{detect_floats, join_caption_labels, table_caption_votes};
use crate::analysis::furniture::remove_furniture;
use crate::analysis::order::order_page;
use crate::analysis::page::{PageData, compute_stats};
use crate::backend::{FontInfo, RawPage};
use crate::model::*;

/// Options of the layout analysis.
#[derive(Debug, Clone, Copy)]
pub struct AnalyzeOptions {
    /// Reconstruct the row/column structure of tables.
    pub tables: bool,
    /// Worker threads for reading the pages (0 = one per CPU).
    pub jobs: usize,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self { tables: true, jobs: 0 }
    }
}

pub struct Analysis {
    pub doc: Document,
    /// Lines that belong to no block (running heads, page numbers, equation
    /// numbers), per page. The position-preserving renderers draw them.
    pub loose: Vec<Vec<PLine>>,
    /// Rotated text, per page.
    pub rotated: Vec<Vec<RotatedRun>>,
}

pub fn analyze(raw_pages: Vec<RawPage>, fonts: &[FontInfo], mut meta: Meta, opts: AnalyzeOptions) -> Analysis {
    let mut pages: Vec<PageData> = raw_pages.into_iter().map(PageData::from_raw).collect();
    let stats = compute_stats(&pages);
    let furniture = remove_furniture(&mut pages);
    meta.arxiv_id = meta.arxiv_id.or(furniture.arxiv_id);
    meta.arxiv_stamp = furniture.arxiv_stamp.clone();

    // --- per-page layout ---
    let mut nodes: Vec<Node> = Vec::new();
    for page in pages.iter_mut() {
        join_caption_labels(page);
    }
    // Convention for table captions (above or below the table).
    let (mut votes_above, mut votes_below) = (0, 0);
    for page in pages.iter() {
        let (a, b) = table_caption_votes(page, fonts);
        votes_above += a;
        votes_below += b;
    }
    let table_caption_above = if votes_above > 2 * votes_below {
        Some(true)
    } else if votes_below > 2 * votes_above {
        Some(false)
    } else {
        None
    };

    for page in pages.iter_mut() {
        let floats = detect_floats(page, &stats, fonts, table_caption_above);

        let leaves = order_page(&page.lines, &floats, &stats);
        let leaf_bounds: Vec<(f32, f32)> = leaves.iter().map(|l| (l.bbox.x0, l.bbox.x1)).collect();
        for rb in build_blocks(page, &leaves, &stats, fonts) {
            let (kind, loc) = match rb {
                RawBlock::Text(t) => {
                    let loc = t.loc;
                    (NodeKind::Text(t), loc)
                }
                RawBlock::Footnote(t) => {
                    let loc = t.loc;
                    (NodeKind::Footnote(t), loc)
                }
                RawBlock::Equation(e) => {
                    let loc = e.loc;
                    (NodeKind::Equation(e), loc)
                }
                RawBlock::Float { idx, loc } => (NodeKind::Float(floats[idx].clone()), loc),
            };
            let (leaf_left, leaf_right) = leaf_bounds[loc.leaf];
            nodes.push(Node {
                kind,
                loc,
                class: Class::Unknown,
                tail: (loc.page, loc.leaf),
                leaf_left,
                leaf_right,
                strip_prefix: 0,
            });
        }
    }

    classify_front_matter(&mut nodes, &pages, &stats, fonts);
    classify_body(&mut nodes, &stats, fonts);
    split_references(&mut nodes);
    merge_paragraphs(&mut nodes, &stats);

    let rotated = pages
        .iter()
        .map(|p| {
            p.rotated
                .iter()
                .filter(|r| !r.text.contains('\u{FFFD}') && !r.text.trim().is_empty())
                .map(|r| {
                    let f = &fonts[r.font as usize];
                    let mut style = 0;
                    if f.bold {
                        style |= BOLD;
                    }
                    if f.italic {
                        style |= ITALIC;
                    }
                    if f.mono {
                        style |= MONO;
                    }
                    if f.sans {
                        style |= SANS;
                    }
                    RotatedRun {
                        text: r.text.clone(),
                        x: r.x,
                        y: r.y,
                        angle: r.angle,
                        size: r.size,
                        length: r.length,
                        style,
                        color: r.color,
                        origins: r.origins.clone(),
                    }
                })
                .collect()
        })
        .collect();
    let (doc, loose) = build_document(nodes, &pages, &stats, fonts, meta, furniture.lines, opts);
    Analysis { doc, loose, rotated }
}
