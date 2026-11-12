//! Hard mode: a facsimile of the PDF. Each page is drawn exactly as in the
//! original (vector SVG or PNG) and overlaid with an invisible, selectable
//! text layer whose DOM is organised by logical block, so that the text of a
//! paragraph is contiguous even though it is positioned line by line.

use super::*;
use crate::layout::Line;

const CSS: &str = include_str!("hard.css");
const JS: &str = include_str!("hard.js");

fn line_span(line: &Line, text: &str, pw: f32, ph: f32) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let top = (line.baseline - 0.8 * line.size) / ph * 100.0;
    format!(
        "<span class=\"l\" style=\"left:{:.3}%;top:{:.3}%;font-size:{:.3}cqw\" data-w=\"{:.3}\">{}</span>",
        line.bbox.x0 / pw * 100.0,
        top,
        line.size / pw * 100.0,
        line.bbox.width() / pw * 100.0,
        esc(text)
    )
}

fn kind_name(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Title => "title",
        BlockKind::Authors => "authors",
        BlockKind::Note => "note",
        BlockKind::Heading { .. } => "heading",
        BlockKind::Paragraph { .. } => "paragraph",
        BlockKind::ListItem { .. } => "list-item",
        BlockKind::Equation { .. } => "equation",
        BlockKind::Figure { .. } => "caption",
        BlockKind::Footnote { .. } => "footnote",
        BlockKind::Reference { .. } => "reference",
    }
}

pub(crate) fn render(analysis: &Analysis, images: &mut ImageStore<'_>) -> String {
    let doc = &analysis.doc;
    let title = doc_title(doc);
    let mut pages = String::new();
    for (pi, p) in doc.pages.iter().enumerate() {
        let (pw, ph) = (p.width, p.height);
        let mut layer = String::new();
        for b in &doc.blocks {
            // A block may continue onto this page from an earlier one.
            if !b.lines.iter().any(|l| l.page == pi) {
                continue;
            }
            let cont = b.lines.first().is_some_and(|l| l.page != pi);
            let mut spans = String::new();
            for (li, l) in b.lines.iter().enumerate() {
                if l.page != pi {
                    continue;
                }
                let fallback;
                let text = match b.line_texts.get(li) {
                    Some(t) => t.as_str(),
                    None => {
                        fallback = l.text();
                        fallback.as_str()
                    }
                };
                spans.push_str(&line_span(l, text, pw, ph));
            }
            layer.push_str(&format!(
                "<div class=\"b\" data-kind=\"{}\" data-block=\"{}\"{}>{}</div>\n",
                kind_name(&b.kind),
                b.id,
                if cont { " data-continued=\"1\"" } else { "" },
                spans
            ));
        }
        // Text inside figures and tables, running heads, equation numbers.
        let mut spans = String::new();
        for b in doc.blocks.iter().filter(|b| b.inner_lines.iter().any(|l| l.page == pi)) {
            for l in b.inner_lines.iter().filter(|l| l.page == pi) {
                spans.push_str(&line_span(l, &format!("{} ", l.text()), pw, ph));
            }
        }
        if !spans.is_empty() {
            layer.push_str(&format!("<div class=\"b\" data-kind=\"figure-text\">{spans}</div>\n"));
        }
        if let Some(loose) = analysis.loose.get(pi) {
            let mut spans = String::new();
            for l in loose {
                spans.push_str(&line_span(&l.line, &format!("{} ", l.line.text()), pw, ph));
            }
            if !spans.is_empty() {
                layer.push_str(&format!("<div class=\"b\" data-kind=\"page-furniture\">{spans}</div>\n"));
            }
        }
        let bg = format!(
            "<img class=\"bg\" src=\"{}\" alt=\"Page {}\" width=\"{pw}\" height=\"{ph}\">",
            images.page(pi),
            pi + 1
        );
        pages.push_str(&format!(
            "<section class=\"page\" id=\"page-{n}\" data-page=\"{n}\" style=\"--pw:{pw};--ph:{ph}\">\n{bg}\n<div class=\"tl\">\n{layer}</div>\n</section>\n",
            n = pi + 1,
        ));
    }
    let source = doc
        .meta
        .arxiv_id
        .as_ref()
        .map(|id| format!("<a href=\"https://arxiv.org/abs/{0}\" target=\"_blank\" rel=\"noopener noreferrer\">arXiv:{0}</a>", esc(id)))
        .unwrap_or_default();
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
