//! Friendly mode: a single-column, responsive article in the Offprint design
//! (themes Paper, White and Dark, a contents column that can be hidden, a
//! running head, embedded reading fonts).

use super::*;
use base64::Engine;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Style sheet of the mode, embedded in the page.
const CSS: &str = include_str!("friendly.css");
/// Script of the mode (theme, contents, running head, cross references,
/// footnotes in the margin, figure lightbox), embedded at the end of the page.
const JS: &str = include_str!("friendly.js");
/// Script of the book view (pagination, page turning, its sound),
/// embedded after the main script.
const BOOK_JS: &str = include_str!("book.js");
/// The sound of a page turn in the book view (CC0; see `sounds/README.md`).
const PAGE_SOUND: &[u8] = include_bytes!("sounds/page-turn.mp3");
/// The page turn sound for the book view script, as base64 MP3 data.
static PAGE_SOUND_JS: LazyLock<String> = LazyLock::new(|| {
    format!(
        "window.p2hPageSound=\"{}\";",
        base64::engine::general_purpose::STANDARD.encode(PAGE_SOUND)
    )
});
/// Script run in the `<head>` before the first paint: applies the stored
/// theme and contents state so the page does not flash.
const HEAD_JS: &str = "(function(){var r=document.documentElement;try{var t=localStorage.getItem('p2h-theme');\
if(t==='light')t='paper';if(t==='paper'||t==='white'||t==='dark')r.setAttribute('data-theme',t);\
if(localStorage.getItem('p2h-toc')==='hidden')r.setAttribute('data-toc','hidden');\
if(localStorage.getItem('p2h-view')==='book')r.setAttribute('data-view','book');}catch(e){}})();";

/// Literata, upright, variable weight (subset; see `fonts/README.md`).
const LITERATA_ROMAN: &[u8] = include_bytes!("fonts/literata-roman.woff2");
/// Literata, italic, variable weight.
const LITERATA_ITALIC: &[u8] = include_bytes!("fonts/literata-italic.woff2");
/// IBM Plex Sans, variable weight, for captions, tables and the interface.
const PLEX_SANS: &[u8] = include_bytes!("fonts/plex-sans.woff2");
/// IBM Plex Mono regular, for code (embedded only when the paper has code).
const PLEX_MONO_400: &[u8] = include_bytes!("fonts/plex-mono-400.woff2");
/// IBM Plex Mono semibold (keywords of coloured code).
const PLEX_MONO_600: &[u8] = include_bytes!("fonts/plex-mono-600.woff2");
/// IBM Plex Mono italic (comments of coloured code).
const PLEX_MONO_ITALIC: &[u8] = include_bytes!("fonts/plex-mono-400-italic.woff2");

/// Deepest heading level listed in the table of contents: deeper levels
/// would make it too long to scan.
const TOC_MAX_LEVEL: u8 = 3;
/// A contents list with more entries than this gets a filter box.
const TOC_FILTER_MIN_ENTRIES: usize = 40;
/// A heading text repeated this many times or more (a running title
/// mistaken for a heading) is listed in the contents only once.
const TOC_REPEAT_LIMIT: usize = 3;
/// Author groups shown before the "show all" button.
const AUTHORS_SHOWN: usize = 12;

/// The `@font-face` rules of the embedded text fonts, built once.
static FONT_FACES: LazyLock<String> = LazyLock::new(|| {
    [
        font_face("Literata", "normal", "380 700", LITERATA_ROMAN),
        font_face("Literata", "italic", "380 700", LITERATA_ITALIC),
        font_face("IBM Plex Sans", "normal", "400 600", PLEX_SANS),
    ]
    .concat()
});

/// The `@font-face` rules of the code font, built once.
static MONO_FACES: LazyLock<String> = LazyLock::new(|| {
    [
        font_face("IBM Plex Mono", "normal", "400", PLEX_MONO_400),
        font_face("IBM Plex Mono", "normal", "600", PLEX_MONO_600),
        font_face("IBM Plex Mono", "italic", "400", PLEX_MONO_ITALIC),
    ]
    .concat()
});

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
        offprint: true,
    };
    let mut content = render_flow(&front_rest, images, &ctx);
    content.push_str(&render_flow(body, images, &ctx));

    let fonts = match (images.opts.embed_fonts, content.contains("<pre class=\"listing")) {
        (false, _) => String::new(),
        (true, false) => FONT_FACES.clone(),
        (true, true) => format!("{}{}", *FONT_FACES, *MONO_FACES),
    };
    let math = if content.contains("<math ") {
        math_css(images.opts.embed_fonts)
    } else {
        String::new()
    };
    let page = Page {
        title: &title,
        topbar: &topbar(doc, &title),
        toc: &toc_html(body),
        head: &head,
        content: &content,
        fonts: &fonts,
        math: &math,
    };
    page.html()
}

/// The parts of the page, put together by [`Page::html`].
struct Page<'a> {
    /// Title of the paper (plain text).
    title: &'a str,
    /// The running head.
    topbar: &'a str,
    /// The contents column (may be empty).
    toc: &'a str,
    /// The head of the article (kicker, title, authors, notes).
    head: &'a str,
    /// The body of the article.
    content: &'a str,
    /// `@font-face` rules of the embedded fonts (empty with system fonts).
    fonts: &'a str,
    /// Style of the MathML formulas, with the embedded math font (empty
    /// when the paper has no MathML).
    math: &'a str,
}

impl Page<'_> {
    /// The complete HTML document.
    fn html(&self) -> String {
        let embedded: Vec<&str> = [
            (!self.fonts.is_empty()).then_some("Literata and IBM Plex"),
            self.math.contains("@font-face").then_some("STIX Two Math"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let colophon = if embedded.is_empty() {
            "Converted from PDF by paper2html · friendly mode".to_string()
        } else {
            format!(
                "Converted from PDF by paper2html · friendly mode · {}, SIL Open Font License 1.1",
                embedded.join(", ")
            )
        };
        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="paper2html (friendly)">
<title>{title}</title>
<script>{HEAD_JS}</script>
<style>
{fonts}{math}{CSS}</style>
</head>
<body class="friendly">
{topbar}<div class="layout">
{toc}<article class="article" id="article">
<header class="paper-head">
{head}</header>
{content}<footer class="colophon">{colophon}</footer>
</article>
</div>
<dialog class="drawer" id="toc-drawer" aria-label="Contents"><button class="btn drawer-close" type="button" aria-label="Close contents">✕</button></dialog>
<button class="return" type="button" hidden><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13 8H3M7 4L3 8l4 4" stroke="currentColor" stroke-width="1.5" fill="none"/></svg><span>Back</span></button>
<div id="lightbox" role="dialog" aria-label="Enlarged figure"><img alt=""></div>
<div class="sr" id="live" aria-live="polite"></div>
<script>
{JS}</script>
<script>{sound}</script>
<script>
{BOOK_JS}</script>
</body>
</html>
"#,
            title = esc(self.title),
            sound = PAGE_SOUND_JS.as_str(),
            fonts = self.fonts,
            math = self.math,
            topbar = self.topbar,
            toc = self.toc,
            head = self.head,
            content = self.content,
        )
    }
}

/// The running head: contents toggle, title and current section, link to
/// the PDF, theme control (a segmented control, a menu on phones) and the
/// reading progress rule.
fn topbar(doc: &Document, title: &str) -> String {
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
    let toggle = "<button class=\"btn toc-toggle\" id=\"toc-toggle\" type=\"button\" aria-expanded=\"true\" aria-controls=\"toc\">\
<svg viewBox=\"0 0 16 16\" aria-hidden=\"true\"><rect x=\"1.75\" y=\"2.75\" width=\"12.5\" height=\"10.5\" rx=\"1\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\"/>\
<rect class=\"pane\" x=\"2.5\" y=\"3.5\" width=\"3.75\" height=\"9\" fill=\"currentColor\"/><path d=\"M6.25 3.5v9\" stroke=\"currentColor\" stroke-width=\"1.5\"/></svg>\
<span class=\"lbl\">Contents</span></button>";
    let themes = ["paper", "white", "dark"];
    let seg: String = themes
        .iter()
        .map(|theme| {
            format!(
                "<button type=\"button\" data-theme-pick=\"{theme}\" aria-pressed=\"false\">{}</button>",
                capitalised(theme)
            )
        })
        .collect();
    let radios: String = themes
        .iter()
        .map(|theme| {
            format!(
                "<label><input type=\"radio\" name=\"p2h-theme\" value=\"{theme}\">{}</label>",
                capitalised(theme)
            )
        })
        .collect();
    format!(
        "<header class=\"topbar\">{toggle}<div class=\"rh\"><span class=\"rh-title\">{}</span><span class=\"rh-sec\"></span></div>{source}\
<div class=\"seg view\" role=\"group\" aria-label=\"View\"><button type=\"button\" data-view-pick=\"scroll\" aria-pressed=\"true\">Scroll</button>\
<button type=\"button\" data-view-pick=\"book\" aria-pressed=\"false\">Book</button></div>\
<button class=\"btn view-btn\" type=\"button\" aria-label=\"Book view\" aria-pressed=\"false\">\
<svg viewBox=\"0 0 16 16\" aria-hidden=\"true\"><path d=\"M8 3.5C6.5 2.5 4 2.25 1.75 2.75v9.5C4 11.75 6.5 12 8 13c1.5-1 4-1.25 6.25-.75v-9.5C12 2.25 9.5 2.5 8 3.5zM8 3.5V13\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.4\" stroke-linejoin=\"round\"/></svg></button>\
<div class=\"seg theme\" role=\"group\" aria-label=\"Theme\">{seg}</div>\
<button class=\"btn theme-btn\" type=\"button\" aria-label=\"Theme\" aria-expanded=\"false\" aria-controls=\"theme-menu\">\
<svg viewBox=\"0 0 16 16\" aria-hidden=\"true\"><circle cx=\"8\" cy=\"8\" r=\"6\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\"/><path d=\"M8 2a6 6 0 0 1 0 12z\" fill=\"currentColor\"/></svg></button>\
<fieldset class=\"theme-menu\" id=\"theme-menu\" hidden><legend>Theme</legend>{radios}</fieldset>\
<div id=\"progress\"></div></header>\n",
        esc(title)
    )
}

/// `paper` → `Paper`.
fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The head of the article: a kicker line (arXiv link, category, date, page
/// count), the title, the author groups and the notes of the front matter.
fn paper_head(doc: &Document, title: &str, front: &[&Block]) -> String {
    let mut kicker = Vec::new();
    if let Some(id) = &doc.meta.arxiv_id {
        kicker.push(format!(
            "<a href=\"https://arxiv.org/abs/{0}\" target=\"_blank\" rel=\"noopener noreferrer\">arXiv:{0}</a>",
            esc(id)
        ));
    }
    if let Some(stamp) = &doc.meta.arxiv_stamp {
        // "arXiv:1706.03762v7 [cs.CL] 2 Aug 2023" -> category and date.
        if let Some((_, rest)) = stamp.split_once(' ') {
            let rest = rest.trim();
            match rest.split_once(']') {
                Some((category, date)) => {
                    kicker.push(esc(category.trim_start_matches('[')));
                    if !date.trim().is_empty() {
                        kicker.push(esc(date.trim()));
                    }
                }
                None => kicker.push(esc(rest)),
            }
        }
    }
    kicker.push(format!("{} pages", doc.pages.len()));
    let mut head = format!(
        "<div class=\"kicker\">{}</div>\n<h1 class=\"title\">{}</h1>\n",
        kicker
            .iter()
            .map(|item| format!("<span>{item}</span>"))
            .collect::<Vec<_>>()
            .join("<span class=\"sep\" aria-hidden=\"true\">/</span>"),
        esc(title)
    );
    head.push_str(&authors_grid(front));
    for block in front.iter().filter(|block| matches!(block.kind, BlockKind::Note)) {
        head.push_str(&format!("<p class=\"note\">{}</p>\n", spans_html(&block.spans)));
    }
    head
}

/// The author groups as a grid: the first [`AUTHORS_SHOWN`] visible, the
/// others hidden behind a "show all" button.
fn authors_grid(front: &[&Block]) -> String {
    let groups: Vec<&Block> = front
        .iter()
        .copied()
        .filter(|block| matches!(block.kind, BlockKind::Authors))
        .collect();
    if groups.is_empty() {
        return String::new();
    }
    let mut out = String::from("<div class=\"authors\">");
    for (idx, block) in groups.iter().enumerate() {
        let hidden = if idx >= AUTHORS_SHOWN { " hidden" } else { "" };
        out.push_str(&format!("<div class=\"author\"{hidden}>"));
        for (row_idx, row) in block.rows.iter().enumerate() {
            let cls = if row_idx == 0 { "au-name" } else { "au-line" };
            out.push_str(&format!("<div class=\"{cls}\">{}</div>", spans_html(row)));
        }
        out.push_str("</div>");
    }
    if groups.len() > AUTHORS_SHOWN {
        out.push_str(&format!(
            "<button class=\"au-more\" type=\"button\" aria-expanded=\"false\">Show all {} author groups</button>",
            groups.len()
        ));
    }
    out.push_str("</div>\n");
    out
}

/// The contents column: the headings of the body down to [`TOC_MAX_LEVEL`]
/// that are link targets, with their section numbers set apart, or an empty
/// string when there are none. Headings repeated [`TOC_REPEAT_LIMIT`] times
/// or more are listed once; long lists get a filter box.
fn toc_html(body: &[&Block]) -> String {
    let headings: Vec<(&Block, u8, &String)> = body
        .iter()
        .filter_map(|block| match (&block.kind, &block.anchor) {
            (BlockKind::Heading { level }, Some(anchor)) if *level <= TOC_MAX_LEVEL => Some((*block, *level, anchor)),
            _ => None,
        })
        .collect();
    // The text of a heading without its footnote marks ("scaling¹¹").
    let title = |block: &Block| -> String {
        block
            .spans
            .iter()
            .filter(|span| !(span.style & SUP != 0 && span.href.as_deref().is_some_and(|href| href.starts_with("#fn"))))
            .map(|span| span.text.as_str())
            .collect::<String>()
            .trim()
            .to_string()
    };
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (block, _, _) in &headings {
        *counts.entry(title(block)).or_default() += 1;
    }
    let mut listed: HashMap<String, bool> = HashMap::new();
    let mut items = String::new();
    let mut entries = 0usize;
    for (block, level, anchor) in headings {
        let text = title(block);
        if counts[&text] >= TOC_REPEAT_LIMIT && listed.insert(text.clone(), true).is_some() {
            continue;
        }
        let (number, name) = split_section_number(&text);
        items.push_str(&format!(
            "<li class=\"l{level}\"><a href=\"#{}\"><span class=\"n\">{}</span>{}</a></li>\n",
            esc(anchor),
            esc(number),
            esc(name)
        ));
        entries += 1;
    }
    if items.is_empty() {
        return String::new();
    }
    let filter = if entries > TOC_FILTER_MIN_ENTRIES {
        "<input class=\"toc-filter\" type=\"search\" placeholder=\"Filter\" aria-label=\"Filter contents\">"
    } else {
        ""
    };
    format!(
        "<nav class=\"toc\" id=\"toc\" aria-label=\"Contents\"><p class=\"toc-title\">Contents</p>{filter}<ol>\n{items}</ol></nav>\n"
    )
}

/// Splits a heading text into its section number (may be empty) and name.
fn split_section_number(text: &str) -> (&str, &str) {
    match text.split_once(char::is_whitespace) {
        Some((number, name))
            if number
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit() || ch.is_ascii_uppercase())
                && number
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || ch == '.' || ch.is_ascii_uppercase())
                && (number.chars().any(|ch| ch.is_ascii_digit()) || number.ends_with('.'))
                && !name.is_empty() =>
        {
            (number, name.trim_start())
        }
        _ => ("", text),
    }
}
