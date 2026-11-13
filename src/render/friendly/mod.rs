//! Friendly mode: a single-column, responsive article.

use super::*;

/// Style sheet of the mode, embedded in the page.
const CSS: &str = include_str!("friendly.css");
/// Script of the mode (reading progress, theme, figure lightbox).
const JS: &str = include_str!("friendly.js");

/// Deepest heading level listed in the table of contents: deeper levels
/// would make it too long to scan.
const TOC_MAX_LEVEL: u8 = 3;

/// The `friendly` mode: single-column responsive article.
pub(crate) struct Friendly;

impl Renderer for Friendly {
    fn render(&self, analysis: &Analysis, images: &mut ImageStore<'_>) -> String {
        render(&analysis.doc, images)
    }
}

/// Writes the whole document in friendly mode. Pictures are requested from
/// `images` in reading order (front matter first).
pub(crate) fn render(doc: &Document, images: &mut ImageStore<'_>) -> String {
    let title = doc_title(doc);
    let all: Vec<&Block> = doc.blocks.iter().collect();

    // Front matter: everything up to the last title/author block.
    let front_end = all
        .iter()
        .rposition(|block| matches!(block.kind, BlockKind::Title | BlockKind::Authors))
        .map(|i| i + 1)
        .unwrap_or(0);
    let (front, body) = all.split_at(front_end);

    let head = paper_head(doc, &title, front);
    // Floats or footnotes that sit between title and body are kept.
    let front_rest: Vec<&Block> = front
        .iter()
        .copied()
        .filter(|block| !matches!(block.kind, BlockKind::Title | BlockKind::Authors | BlockKind::Note))
        .collect();

    let ctx = FlowCtx {
        unit: WidthUnit::Em {
            pt_per_em: doc.body_size,
        },
    };
    let mut content = render_flow(&front_rest, images, &ctx);
    content.push_str(&render_flow(body, images, &ctx));

    let toc = toc_html(body);
    let source = doc
        .meta
        .arxiv_id
        .as_ref()
        .map(|id| {
            format!(
                "<a href=\"https://arxiv.org/pdf/{}\" target=\"_blank\" rel=\"noopener noreferrer\">PDF</a>",
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

/// The head of the article: title, author cards, a line of metadata (arXiv
/// link, category and date, page count) and the notes of the front matter.
fn paper_head(doc: &Document, title: &str, front: &[&Block]) -> String {
    let mut head = String::new();
    head.push_str(&format!("<h1>{}</h1>\n", esc(title)));
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
    head.push_str(&format!(
        "<div class=\"meta\">{}</div>\n",
        meta.iter()
            .map(|item| format!("<span>{item}</span>"))
            .collect::<String>()
    ));
    for block in front.iter().filter(|block| matches!(block.kind, BlockKind::Note)) {
        head.push_str(&format!("<p class=\"note\">{}</p>\n", spans_html(&block.spans)));
    }
    head
}

/// The table of contents: the headings of the body down to
/// [`TOC_MAX_LEVEL`] that are link targets, or an empty string when there
/// are none.
fn toc_html(body: &[&Block]) -> String {
    let mut toc = String::new();
    for block in body {
        if let (BlockKind::Heading { level }, Some(anchor)) = (&block.kind, &block.anchor)
            && *level <= TOC_MAX_LEVEL
        {
            toc.push_str(&format!(
                "<li class=\"l{level}\"><a href=\"#{}\">{}</a></li>\n",
                esc(anchor),
                esc(&plain(&block.spans))
            ));
        }
    }
    if toc.is_empty() {
        String::new()
    } else {
        format!(
            "<nav class=\"toc\" aria-label=\"Table of contents\"><p class=\"toc-title\">Contents</p><ol>\n{toc}</ol></nav>\n"
        )
    }
}
