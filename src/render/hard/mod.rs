//! Hard mode: a facsimile of the PDF. Each page is drawn exactly as in the
//! original (vector SVG or PNG) and overlaid with an invisible, selectable
//! text layer whose DOM is organised by logical block, so that the text of a
//! paragraph is contiguous even though it is positioned line by line.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use super::*;
use crate::model::Line;

/// Style sheet of the mode, embedded in the page.
const CSS: &str = include_str!("hard.css");
/// Script of the mode (zoom, line widths), embedded in the page.
const JS: &str = include_str!("hard.js");

/// Distance of the baseline below the top of an invisible text line, in em:
/// the line box is placed so that its text covers the glyphs of the picture.
const LINE_BASELINE_EM: f32 = 0.8;

/// The `hard` mode: facsimile pages with an invisible text layer.
pub(crate) struct Hard;

impl Renderer for Hard {
    fn render(&self, analysis: &Analysis, images: &mut ImageStore<'_>) -> String {
        render(analysis, images)
    }
}

/// Writes the whole document in hard mode. The page pictures are requested
/// from `images`, one per page in page order.
pub(crate) fn render(analysis: &Analysis, images: &mut ImageStore<'_>) -> String {
    let doc = &analysis.doc;
    let title = doc_title(doc);
    let mut pages = String::new();
    for (page_idx, page) in doc.pages.iter().enumerate() {
        pages.push_str(&page_html(analysis, page_idx, page, images));
    }
    let source = arxiv_abs_link(doc);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="paper2html (hard)">
<title>{title}</title>
<style>
{CSS}</style>
</head>
<body class="hard fit">
<div class="topbar"><span class="t">{title}</span>{source}<span>{n} pages</span><button id="zout" type="button" aria-label="Zoom out">−</button><button id="zfit" type="button">Fit</button><button id="zin" type="button" aria-label="Zoom in">+</button></div>
<main class="pages">
{pages}</main>
<script>
{JS}</script>
</body>
</html>
"#,
        title = esc(&title),
        n = doc.pages.len(),
    )
}

/// One page: its picture and the text layer over it.
fn page_html(analysis: &Analysis, page_idx: usize, page: &PageInfo, images: &mut ImageStore<'_>) -> String {
    let doc = &analysis.doc;
    let (pw, ph) = (page.width, page.height);
    let mut layer = String::new();
    for block in &doc.blocks {
        block_layer(block, page_idx, pw, ph, &mut layer);
    }
    figure_text_layer(doc, page_idx, pw, ph, &mut layer);
    if let Some(loose) = analysis.loose.get(page_idx) {
        let mut spans = String::new();
        for loose_line in loose {
            spans.push_str(&line_span(
                &loose_line.line,
                &format!("{} ", loose_line.line.text()),
                pw,
                ph,
            ));
        }
        if !spans.is_empty() {
            layer.push_str(&format!(
                "<div class=\"b\" data-kind=\"page-furniture\">{spans}</div>\n"
            ));
        }
    }
    let bg = format!(
        "<img class=\"bg\" src=\"{}\" alt=\"Page {}\" width=\"{pw}\" height=\"{ph}\" loading=\"lazy\" decoding=\"async\">",
        images.page(page_idx),
        page_idx + 1
    );
    format!(
        "<section class=\"page\" id=\"page-{n}\" data-page=\"{n}\" style=\"--pw:{pw};--ph:{ph};aspect-ratio:{pw}/{ph}\">\n{bg}\n<div class=\"tl\">\n{layer}</div>\n</section>\n",
        n = page_idx + 1,
    )
}

/// Writes the lines of `block` that are on page `page_idx` as one element of
/// the text layer, or nothing when the block has no line there. A block that
/// started on an earlier page is marked as continued.
fn block_layer(block: &Block, page_idx: usize, pw: f32, ph: f32, layer: &mut String) {
    // A block may continue onto this page from an earlier one.
    if !block.lines.iter().any(|line| line.page == page_idx) {
        return;
    }
    let cont = block.lines.first().is_some_and(|line| line.page != page_idx);
    let mut spans = String::new();
    for (line_idx, line) in block.lines.iter().enumerate() {
        if line.page != page_idx {
            continue;
        }
        let fallback;
        let text = match block.line_texts.get(line_idx) {
            Some(text) => text.as_str(),
            None => {
                fallback = line.text();
                fallback.as_str()
            }
        };
        spans.push_str(&line_span(line, text, pw, ph));
    }
    layer.push_str(&format!(
        "<div class=\"b\" data-kind=\"{}\" data-block=\"{}\"{}>{}</div>\n",
        kind_name(&block.kind),
        block.id,
        if cont { " data-continued=\"1\"" } else { "" },
        spans
    ));
}

/// Writes the text inside figures and tables (also running heads and
/// equation numbers) on page `page_idx` as one element of the text layer.
fn figure_text_layer(doc: &Document, page_idx: usize, pw: f32, ph: f32, layer: &mut String) {
    let mut spans = String::new();
    for block in doc
        .blocks
        .iter()
        .filter(|block| block.inner_lines.iter().any(|line| line.page == page_idx))
    {
        for line in block.inner_lines.iter().filter(|line| line.page == page_idx) {
            spans.push_str(&line_span(line, &format!("{} ", line.text()), pw, ph));
        }
    }
    if !spans.is_empty() {
        layer.push_str(&format!("<div class=\"b\" data-kind=\"figure-text\">{spans}</div>\n"));
    }
}

/// One invisible line of the text layer, positioned in percent of the page
/// size (`pw` x `ph` points). Returns an empty string for blank text.
fn line_span(line: &Line, text: &str, pw: f32, ph: f32) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let top = (line.baseline - LINE_BASELINE_EM * line.size) / ph * 100.0;
    format!(
        "<span class=\"l\" style=\"left:{:.3}%;top:{:.3}%;font-size:{:.3}cqw\" data-w=\"{:.3}\">{}</span>",
        line.bbox.x0 / pw * 100.0,
        top,
        line.size / pw * 100.0,
        line.bbox.width() / pw * 100.0,
        esc(text)
    )
}
