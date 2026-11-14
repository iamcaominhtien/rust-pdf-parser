//! Flowing HTML for a sequence of blocks, shared by the medium and friendly modes.

use super::*;
use crate::model::*;
use regex::Regex;
use std::sync::LazyLock;

/// A section number at the start of a heading ("3.2 ", "A.1 ", "IV. "),
/// set apart in the friendly mode so it can hang in the margin.
static SECTION_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\d+(?:\.\d+)*\.?|[A-Z](?:\.\d+)+\.?|[A-Z]\.|[IVXL]+\.)\s+").unwrap());

/// Smallest scale of an equation or a picture of a table in a narrow column
/// (a phone, a book page): text in it stays readable, and the picture
/// scrolls sideways instead.
const LEGIBLE_MIN_SCALE: f32 = 0.7;

/// How image widths are expressed in a given mode.
pub(crate) enum WidthUnit {
    /// Relative to the body font size (`pt_per_em` points per em).
    Em {
        /// Points per em: the body font size of the document.
        pt_per_em: f32,
    },
    /// Relative to the page container width (`page_width` points wide).
    Cqw {
        /// Width of the page in points.
        page_width: f32,
    },
}

impl WidthUnit {
    /// The CSS `width` of an image that is `width_pt` points wide in the
    /// PDF, never wider than its container. A `legible` image (equations,
    /// pictures of tables) shrinks to fit only down to
    /// [`LEGIBLE_MIN_SCALE`] of its size; beyond that its container scrolls.
    fn css(&self, width_pt: f32, legible: bool) -> String {
        match self {
            WidthUnit::Em { pt_per_em } if legible => {
                let width = width_pt / pt_per_em;
                format!(
                    "width:{width:.2}em;max-width:max(100%,{:.2}em)",
                    width * LEGIBLE_MIN_SCALE
                )
            }
            WidthUnit::Em { pt_per_em } => format!("width:{:.2}em;max-width:100%", width_pt / pt_per_em),
            WidthUnit::Cqw { page_width } => format!("width:{:.2}cqw;max-width:100%", width_pt / page_width * 100.0),
        }
    }
}

/// What a mode needs to know to write flowing blocks.
pub(crate) struct FlowCtx {
    /// How image widths are expressed.
    pub unit: WidthUnit,
    /// Write the markup of the Offprint design (friendly mode): hanging
    /// section numbers, reference and list labels, figure plates, caption
    /// labels without punctuation, numeric table columns.
    pub offprint: bool,
}

/// A run of consecutive blocks that share a container element.
#[derive(PartialEq, Clone, Copy)]
enum Group {
    /// No container.
    None,
    /// List items, in a `<ul>`.
    List,
    /// Bibliography entries, in an `<ol>`.
    Refs,
    /// Paragraphs of the abstract, in a `<section>`.
    Abstract,
}

/// The HTML heading tag of a heading level (`h1` is the paper title).
pub(crate) fn heading_tag(level: u8) -> &'static str {
    match level {
        0 | 1 => "h2",
        2 => "h3",
        3 => "h4",
        _ => "h5",
    }
}

/// Renders a sequence of blocks as flowing HTML, grouping list items,
/// references and abstract paragraphs. Pictures are requested from `images`
/// in the order of the blocks.
pub(crate) fn render_flow(blocks: &[&Block], images: &mut ImageStore<'_>, ctx: &FlowCtx) -> String {
    let mut out = String::new();
    let mut group = Group::None;
    let mut block_idx = 0;
    while block_idx < blocks.len() {
        let block = blocks[block_idx];
        if ctx.offprint
            && let Some(end) = code_run(blocks, block_idx)
        {
            close_group(group, &mut out);
            group = Group::None;
            let run = &blocks[block_idx..end];
            out.push_str(&code::listing_html(&code::listing_lines(run), "", &id_attr(run[0])));
            block_idx = end;
            continue;
        }
        let want = group_of(block, group);
        if want != group {
            close_group(group, &mut out);
            open_group(want, blocks, block_idx, &mut out);
            group = want;
        }
        // In the friendly mode an equation whose number was read as a
        // paragraph of its own ("(2)") takes that number back.
        if ctx.offprint
            && let BlockKind::Equation { image, number: None } = &block.kind
            && let Some(number) = blocks.get(block_idx + 1).and_then(|next| stray_equation_number(next))
        {
            out.push_str(&equation_html(
                block,
                image,
                &Some(number),
                &id_attr(block),
                images,
                ctx,
            ));
            block_idx += 2;
            continue;
        }
        block_html(block, images, ctx, &mut out);
        block_idx += 1;
    }
    close_group(group, &mut out);
    out
}

/// The equation number written as a paragraph of its own: a paragraph whose
/// whole text is a number in parentheses, "(2)" or "(2.1)".
fn stray_equation_number(block: &Block) -> Option<String> {
    if !matches!(block.kind, BlockKind::Paragraph { in_abstract: false }) {
        return None;
    }
    let text = plain(&block.spans);
    let text = text.trim();
    let inner = text.strip_prefix('(')?.strip_suffix(')')?;
    let digits = inner
        .split('.')
        .all(|part| !part.is_empty() && part.len() <= 3 && part.bytes().all(|b| b.is_ascii_digit()));
    digits.then(|| text.to_string())
}

/// The end of the run of code paragraphs starting at `blocks[start]`, when
/// the run has at least two lines (one monospaced line stays inline code).
fn code_run(blocks: &[&Block], start: usize) -> Option<usize> {
    let end = start
        + blocks[start..]
            .iter()
            .take_while(|block| code::is_code_paragraph(block))
            .count();
    let lines: usize = blocks[start..end].iter().map(|block| block.lines.len()).sum();
    (lines >= 2).then_some(end)
}

/// Author groups as cards.
pub(crate) fn authors_html(blocks: &[&Block]) -> String {
    let cards: Vec<String> = blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Authors))
        .map(|block| {
            let rows: Vec<String> = block
                .rows
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    format!(
                        "<div class=\"{}\">{}</div>",
                        if i == 0 { "au-name" } else { "au-line" },
                        spans_html(row)
                    )
                })
                .collect();
            format!("<div class=\"author\">{}</div>", rows.join(""))
        })
        .collect();
    if cards.is_empty() {
        String::new()
    } else {
        format!("<div class=\"authors\">{}</div>\n", cards.join("\n"))
    }
}

/// The group a block belongs to, given the group that is open (`current`):
/// floats, equations and footnotes do not interrupt an abstract.
fn group_of(block: &Block, current: Group) -> Group {
    match &block.kind {
        BlockKind::ListItem { .. } => Group::List,
        BlockKind::Reference { .. } => Group::Refs,
        BlockKind::Paragraph { in_abstract: true } => Group::Abstract,
        // Floats, equations and footnotes do not interrupt a group.
        BlockKind::Equation { .. } if current == Group::Abstract => current,
        _ => Group::None,
    }
}

/// Writes the closing tag of a group.
fn close_group(group: Group, out: &mut String) {
    match group {
        Group::List => out.push_str("</ul>\n"),
        Group::Refs => out.push_str("</ol>\n"),
        Group::Abstract => out.push_str("</section>\n"),
        Group::None => {}
    }
}

/// Writes the opening tag of a group that starts at `blocks[block_idx]`. An
/// abstract gets an "Abstract" label unless the block before it is a heading
/// saying so.
fn open_group(group: Group, blocks: &[&Block], block_idx: usize, out: &mut String) {
    match group {
        Group::List => out.push_str("<ul class=\"list\">\n"),
        Group::Refs => out.push_str("<ol class=\"refs\">\n"),
        Group::Abstract => {
            // The "Abstract" heading (if any) was emitted just before.
            let had_heading = block_idx > 0
                && matches!(blocks[block_idx - 1].kind, BlockKind::Heading { .. })
                && is_abstract_text(&blocks[block_idx - 1].spans);
            out.push_str("<section class=\"abstract\">\n");
            if !had_heading {
                out.push_str("<div class=\"abstract-label\">Abstract</div>\n");
            }
        }
        Group::None => {}
    }
}

/// Whether the text of `spans` is the word "Abstract" (any case, with an
/// optional final `.` or `:`).
fn is_abstract_text(spans: &[Span]) -> bool {
    plain(spans)
        .trim_end_matches(['.', ':'])
        .eq_ignore_ascii_case("abstract")
}

/// Writes one block. Titles and author blocks are written by the modes
/// themselves and are skipped here.
fn block_html(block: &Block, images: &mut ImageStore<'_>, ctx: &FlowCtx, out: &mut String) {
    let id = id_attr(block);
    match &block.kind {
        BlockKind::Title | BlockKind::Authors => {}
        BlockKind::Note => out.push_str(&format!("<p class=\"note\">{}</p>\n", spans_html(&block.spans))),
        BlockKind::Heading { level } => {
            let tag = heading_tag(*level);
            let cls = if is_abstract_text(&block.spans) {
                " class=\"abstract-heading\""
            } else {
                ""
            };
            match section_number(&block.spans).filter(|_| ctx.offprint && cls.is_empty()) {
                Some((number, rest)) => out.push_str(&format!(
                    "<{tag}{id} class=\"hang\"><span class=\"secno\">{}</span>{}</{tag}>\n",
                    esc(&number),
                    spans_html(&rest)
                )),
                None => out.push_str(&format!("<{tag}{id}{cls}>{}</{tag}>\n", spans_html(&block.spans))),
            }
        }
        BlockKind::Paragraph { .. } => out.push_str(&format!("<p{id}>{}</p>\n", spans_html(&block.spans))),
        BlockKind::ListItem { marker } => {
            let cls = if marker.is_some() { " class=\"labeled\"" } else { "" };
            let body = match marker.as_deref().filter(|_| ctx.offprint) {
                Some(marker) => labelled_html(&block.spans, marker, "lbl"),
                None => spans_html(&block.spans),
            };
            out.push_str(&format!("<li{id}{cls}>{body}</li>\n"));
        }
        BlockKind::Reference { label } => {
            let body = match label.as_deref().filter(|_| ctx.offprint) {
                Some(label) => reference_html(&block.spans, label),
                None => spans_html(&block.spans),
            };
            out.push_str(&format!("<li{id}>{body}</li>\n"));
        }
        BlockKind::Footnote { marker } => out.push_str(&footnote_html(block, marker, &id, ctx)),
        BlockKind::Equation { image, number } => out.push_str(&equation_html(block, image, number, &id, images, ctx)),
        BlockKind::Figure { kind, image, label } => {
            out.push_str(&figure_html(block, *kind, image, label, &id, images, ctx));
        }
    }
}

/// A footnote, with its marker (if any) as a superscript (in the friendly
/// mode, as a rubric label).
fn footnote_html(block: &Block, marker: &str, id: &str, ctx: &FlowCtx) -> String {
    let mark = if marker.is_empty() {
        String::new()
    } else if ctx.offprint {
        format!("<span class=\"fn-mark\">{}</span>", esc(marker))
    } else {
        format!("<sup class=\"fn-mark\">{}</sup> ", esc(marker))
    };
    format!(
        "<aside class=\"footnote\"{id} role=\"note\">{mark}{}</aside>\n",
        spans_html(&block.spans)
    )
}

/// A displayed equation: its picture, with the text as alt text, and its
/// number.
fn equation_html(
    block: &Block,
    image: &ImageSpec,
    number: &Option<String>,
    id: &str,
    images: &mut ImageStore<'_>,
    ctx: &FlowCtx,
) -> String {
    let alt = esc(&plain(&block.spans));
    let img = format!(
        "<img src=\"{}\" alt=\"{alt}\"{} style=\"{}\" loading=\"lazy\">",
        images.region(image, "eq"),
        images.region_size_attrs(image),
        ctx.unit.css(image.rect.width(), true)
    );
    let num = number
        .as_ref()
        .map(|number| format!("<span class=\"eqno\">{}</span>", esc(number)))
        .unwrap_or_default();
    format!("<div class=\"eq\"{id}><span class=\"eq-body\">{img}</span>{num}</div>\n")
}

/// A float: its picture, or the reconstructed table instead of the picture
/// of a table, and its caption on the side it had in the paper.
fn figure_html(
    block: &Block,
    kind: FloatKind,
    image: &Option<ImageSpec>,
    label: &Option<String>,
    id: &str,
    images: &mut ImageStore<'_>,
    ctx: &FlowCtx,
) -> String {
    let cls = float_class(kind);
    // A float whose text is all code is a listing.
    if ctx.offprint
        && kind == FloatKind::Algorithm
        && block.table.is_none()
        && let Some(lines) = code::float_listing_lines(block)
    {
        return code::listing_html(&lines, &caption_html_bare(&block.spans), id);
    }
    let alt = esc(&figure_alt(block, label));
    // An algorithm whose text reads as pseudocode is rebuilt as text.
    let algo = (ctx.offprint && kind == FloatKind::Algorithm && block.table.is_none())
        .then(|| code::algorithm_html(block))
        .flatten();
    let img = image
        .as_ref()
        .filter(|_| block.table.is_none() && algo.is_none())
        .map(|spec| (images.region(spec, cls), images.region_size_attrs(spec), spec))
        .map(|(src, size, spec)| {
            // The medium mode shows figures at their own pixel width on
            // phones (`--w`), known before the picture loads.
            let own = if ctx.offprint {
                String::new()
            } else {
                format!(";--w:{}px", images.region_size(spec).0)
            };
            let legible = matches!(kind, FloatKind::Table | FloatKind::Algorithm);
            format!(
                "<img src=\"{src}\" alt=\"{alt}\"{size} style=\"{}{own}\" loading=\"lazy\">",
                ctx.unit.css(spec.rect.width(), legible)
            )
        })
        .map(|img| match kind {
            FloatKind::Figure | FloatKind::Graphic if ctx.offprint => format!("<div class=\"plate\">{img}</div>"),
            // A picture of a table scrolls like a rebuilt table when the
            // column is too narrow for it.
            FloatKind::Table | FloatKind::Algorithm if ctx.offprint => format!("<div class=\"tbl-wrap\">{img}</div>"),
            _ => img,
        })
        .unwrap_or_default();
    // A reconstructed table replaces the picture of the table.
    let img = match (&block.table, algo) {
        (_, Some(algo)) => algo,
        (Some(table), _) if ctx.offprint => format!("<div class=\"tbl-wrap\">{}</div>", table_html_numeric(table)),
        (Some(table), _) => format!("<div class=\"tbl-wrap\">{}</div>", table_html(table)),
        (None, None) => img,
    };
    let cap = if block.spans.is_empty() {
        String::new()
    } else if ctx.offprint {
        format!("<figcaption>{}</figcaption>", caption_html_bare(&block.spans))
    } else {
        format!("<figcaption>{}</figcaption>", caption_html(&block.spans))
    };
    // Keep the caption on the side it had in the paper.
    let caption_first = match (image, block.lines.first()) {
        _ if img.starts_with("<div class=\"algo\"") => true,
        (Some(spec), Some(line)) => line.bbox.cy() < spec.rect.cy(),
        _ => false,
    };
    let inner = if caption_first {
        format!("{cap}{img}")
    } else {
        format!("{img}{cap}")
    };
    format!("<figure class=\"float {cls}\"{id}>{inner}</figure>\n")
}

/// The section number at the start of a heading and the rest of its spans,
/// or `None` when the heading has no number.
fn section_number(spans: &[Span]) -> Option<(String, Vec<Span>)> {
    let first = spans.first()?;
    let found = SECTION_NUMBER.find(&first.text)?;
    if found.end() == first.text.len() && spans.len() == 1 {
        return None;
    }
    let mut rest = spans.to_vec();
    rest[0].text = first.text[found.end()..].to_string();
    Some((found.as_str().trim_end().to_string(), rest))
}

/// A list item whose text starts with its `marker` ("(1)", "a."): the
/// marker in a `<span class="{class}">`, then the rest of the text.
/// Without the marker at the start, the plain text.
fn labelled_html(spans: &[Span], marker: &str, class: &str) -> String {
    match spans.first() {
        Some(first) if !marker.is_empty() && first.text.starts_with(marker) => {
            let mut rest = spans.to_vec();
            rest[0].text = first.text[marker.len()..].trim_start().to_string();
            format!("<span class=\"{class}\">{}</span>{}", esc(marker), spans_html(&rest))
        }
        _ => spans_html(spans),
    }
}

/// A bibliography entry with its label ("[12]" or "12.") set apart in a
/// `<span class="rn">`.
fn reference_html(spans: &[Span], label: &str) -> String {
    let Some(first) = spans.first() else {
        return String::new();
    };
    let text = first.text.as_str();
    for printed in [format!("[{label}]"), format!("{label}."), label.to_string()] {
        if let Some(rest) = text.strip_prefix(printed.as_str()) {
            let mut rest_spans = spans.to_vec();
            rest_spans[0].text = rest.trim_start().to_string();
            return format!("<span class=\"rn\">{}</span>{}", esc(&printed), spans_html(&rest_spans));
        }
    }
    spans_html(spans)
}
