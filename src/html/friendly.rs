//! Friendly mode: a single-column, responsive article.

use super::*;

const CSS: &str = include_str!("friendly.css");
const JS: &str = include_str!("friendly.js");

pub(crate) fn render(doc: &Document, images: &mut ImageStore<'_>) -> String {
    let title = doc_title(doc);
    let all: Vec<&Block> = doc.blocks.iter().collect();

    // Front matter: everything up to the last title/author block.
    let front_end = all
        .iter()
        .rposition(|b| matches!(b.kind, BlockKind::Title | BlockKind::Authors))
        .map(|i| i + 1)
        .unwrap_or(0);
    let (front, body) = all.split_at(front_end);

    let mut head = String::new();
    head.push_str(&format!("<h1>{}</h1>\n", esc(&title)));
    head.push_str(&authors_html(front));
    let mut meta = Vec::new();
    if let Some(id) = &doc.meta.arxiv_id {
        meta.push(format!(
            "<a href=\"https://arxiv.org/abs/{0}\" target=\"_blank\" rel=\"noopener noreferrer\">arXiv:{0}</a>",
            esc(id)
        ));
    }
    if let Some(stamp) = &doc.meta.arxiv_stamp {
        // "arXiv:1706.03762v7 [cs.CL] 2 Aug 2023" -> category and date.
        if let Some((_, rest)) = stamp.split_once(' ') {
            meta.push(esc(rest.trim()));
        }
    }
    meta.push(format!("{} pages", doc.pages.len()));
    head.push_str(&format!("<div class=\"meta\">{}</div>\n", meta.iter().map(|m| format!("<span>{m}</span>")).collect::<String>()));
    for b in front.iter().filter(|b| matches!(b.kind, BlockKind::Note)) {
        head.push_str(&format!("<p class=\"note\">{}</p>\n", spans_html(&b.spans)));
    }
    // Floats or footnotes that sit between title and body are kept.
    let front_rest: Vec<&Block> = front
        .iter()
        .copied()
        .filter(|b| !matches!(b.kind, BlockKind::Title | BlockKind::Authors | BlockKind::Note))
        .collect();

    let ctx = FlowCtx { unit: WidthUnit::Em { pt_per_em: doc.body_size } };
    let mut content = render_flow(&front_rest, images, &ctx);
    content.push_str(&render_flow(body, images, &ctx));

    let mut toc = String::new();
    for b in body {
        if let (BlockKind::Heading { level }, Some(a)) = (&b.kind, &b.anchor) {
            if *level <= 3 {
                toc.push_str(&format!(
                    "<li class=\"l{level}\"><a href=\"#{}\">{}</a></li>\n",
                    esc(a),
                    esc(&plain(&b.spans))
                ));
            }
        }
    }
    let toc = if toc.is_empty() {
        String::new()
    } else {
        format!("<nav class=\"toc\" aria-label=\"Table of contents\"><p class=\"toc-title\">Contents</p><ol>\n{toc}</ol></nav>\n")
    };
    let source = doc
        .meta
        .arxiv_id
        .as_ref()
        .map(|id| format!("<a href=\"https://arxiv.org/pdf/{}\" target=\"_blank\" rel=\"noopener noreferrer\">PDF</a>", esc(id)))
        .unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="paper2html (friendly)">
<title>{title}</title>
<style>
{CSS}</style>
</head>
<body class="friendly">
<div id="progress"></div>
<div class="topbar"><span class="t">{title}</span>{source}<button id="theme" type="button" aria-label="Toggle dark mode">◐ Theme</button></div>
<div class="layout">
{toc}<main>
<article>
<header class="paper-head">
{head}</header>
{content}<footer class="colophon">Converted from PDF by paper2html · friendly mode</footer>
</article>
</main>
</div>
<div id="lightbox" role="dialog" aria-label="Enlarged figure"><img alt=""></div>
<script>
{JS}</script>
</body>
</html>
"#,
        title = esc(&title),
    )
}
