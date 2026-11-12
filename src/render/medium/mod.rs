//! Medium mode: the page and column structure of the paper is kept, but the
//! text is real, reflowed HTML and paragraphs are never cut at column or page
//! boundaries.

use super::*;
use crate::common::geom::median;

const CSS: &str = include_str!("medium.css");

pub(crate) fn render(doc: &Document, images: &mut ImageStore<'_>) -> String {
    let title = doc_title(doc);

    // Page margins, estimated from the extent of the content.
    let mut lefts = Vec::new();
    let mut tops = Vec::new();
    for (pi, p) in doc.pages.iter().enumerate() {
        let on_page: Vec<&Block> = doc.blocks.iter().filter(|b| b.page == pi).collect();
        if on_page.len() < 2 {
            continue;
        }
        let x0 = on_page.iter().map(|b| b.bbox.x0).fold(f32::INFINITY, f32::min);
        let x1 = on_page.iter().map(|b| b.bbox.x1).fold(f32::NEG_INFINITY, f32::max);
        let y0 = on_page.iter().map(|b| b.bbox.y0).fold(f32::INFINITY, f32::min);
        lefts.push(x0.min(p.width - x1));
        tops.push(y0);
    }
    let mx = median(&lefts, 60.0).clamp(18.0, 130.0);
    let my = median(&tops, 60.0).clamp(24.0, 110.0);
    let line_height = (doc.leading / doc.body_size).clamp(1.22, 1.5);

    let mut pages = String::new();
    for (pi, p) in doc.pages.iter().enumerate() {
        let blocks: Vec<&Block> = doc.blocks.iter().filter(|b| b.page == pi).collect();
        let ctx = FlowCtx {
            unit: WidthUnit::Cqw { page_width: p.width },
        };
        let mut inner = String::new();

        let (footnotes, flow): (Vec<&Block>, Vec<&Block>) = blocks
            .iter()
            .copied()
            .partition(|b| matches!(b.kind, BlockKind::Footnote { .. }));

        // Consecutive blocks of the same zone form one container.
        let mut i = 0;
        while i < flow.len() {
            // Adjacent single-column zones form one flow.
            let zone_key = |b: &Block| {
                if b.loc.zone_cols <= 1 {
                    (0, 1)
                } else {
                    (b.loc.zone + 1, b.loc.zone_cols)
                }
            };
            let key = zone_key(flow[i]);
            let mut j = i;
            while j < flow.len() && zone_key(flow[j]) == key {
                j += 1;
            }
            let zone = &flow[i..j];
            let cols = key.1.max(1);
            if cols > 1 {
                inner.push_str(&format!("<div class=\"zone cols\" style=\"column-count:{cols}\">\n"));
            } else {
                inner.push_str("<div class=\"zone\">\n");
            }
            // Title and author runs get their own markup.
            let mut k = 0;
            while k < zone.len() {
                match zone[k].kind {
                    BlockKind::Title => {
                        let mut parts = Vec::new();
                        while k < zone.len() && matches!(zone[k].kind, BlockKind::Title) {
                            parts.push(spans_html(&zone[k].spans));
                            k += 1;
                        }
                        inner.push_str(&format!("<h1 class=\"title\">{}</h1>\n", parts.join(" ")));
                    }
                    BlockKind::Authors => {
                        let start = k;
                        while k < zone.len() && matches!(zone[k].kind, BlockKind::Authors) {
                            k += 1;
                        }
                        inner.push_str(&authors_html(&zone[start..k]));
                    }
                    _ => {
                        let start = k;
                        while k < zone.len() && !matches!(zone[k].kind, BlockKind::Title | BlockKind::Authors) {
                            k += 1;
                        }
                        inner.push_str(&render_flow(&zone[start..k], images, &ctx));
                    }
                }
            }
            inner.push_str("</div>\n");
            i = j;
        }
        if !footnotes.is_empty() {
            inner.push_str("<div class=\"footnotes\">\n");
            inner.push_str(&render_flow(&footnotes, images, &ctx));
            inner.push_str("</div>\n");
        }
        pages.push_str(&format!(
            "<section class=\"page\" id=\"page-{n}\" data-page=\"{n}\" style=\"--pw:{w};--ph:{h}\">\n<div class=\"page-inner\">\n{inner}</div>\n<div class=\"page-no\">{n}</div>\n</section>\n",
            n = pi + 1,
            w = p.width,
            h = p.height,
        ));
    }

    let source = doc
        .meta
        .arxiv_id
        .as_ref()
        .map(|id| {
            format!(
                "<a href=\"https://arxiv.org/abs/{0}\" target=\"_blank\" rel=\"noopener noreferrer\">arXiv:{0}</a>",
                esc(id)
            )
        })
        .unwrap_or_default();
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
