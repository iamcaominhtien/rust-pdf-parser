//! Flowing HTML for a sequence of blocks, shared by the medium and friendly modes.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use super::*;
use crate::model::*;

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
    /// PDF, never wider than its container.
    fn css(&self, width_pt: f32) -> String {
        match self {
            WidthUnit::Em { pt_per_em } => format!("width:min(100%,{:.2}em)", width_pt / pt_per_em),
            WidthUnit::Cqw { page_width } => format!("width:min(100%,{:.2}cqw)", width_pt / page_width * 100.0),
        }
    }
}

/// What a mode needs to know to write flowing blocks.
pub(crate) struct FlowCtx {
    /// How image widths are expressed.
    pub unit: WidthUnit,
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
    for (block_idx, block) in blocks.iter().enumerate() {
        let want = group_of(block, group);
        if want != group {
            close_group(group, &mut out);
            open_group(want, blocks, block_idx, &mut out);
            group = want;
        }
        block_html(block, images, ctx, &mut out);
    }
    close_group(group, &mut out);
    out
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
            out.push_str(&format!("<{tag}{id}{cls}>{}</{tag}>\n", spans_html(&block.spans)));
        }
        BlockKind::Paragraph { .. } => out.push_str(&format!("<p{id}>{}</p>\n", spans_html(&block.spans))),
        BlockKind::ListItem { marker } => {
            let cls = if marker.is_some() { " class=\"labeled\"" } else { "" };
            out.push_str(&format!("<li{id}{cls}>{}</li>\n", spans_html(&block.spans)));
        }
        BlockKind::Reference { .. } => out.push_str(&format!("<li{id}>{}</li>\n", spans_html(&block.spans))),
        BlockKind::Footnote { marker } => out.push_str(&footnote_html(block, marker, &id)),
        BlockKind::Equation { image, number } => out.push_str(&equation_html(block, image, number, &id, images, ctx)),
        BlockKind::Figure { kind, image, label } => {
            out.push_str(&figure_html(block, *kind, image, label, &id, images, ctx));
        }
    }
}

/// A footnote, with its marker (if any) as a superscript.
fn footnote_html(block: &Block, marker: &str, id: &str) -> String {
    let mark = if marker.is_empty() {
        String::new()
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
        "<img src=\"{}\" alt=\"{alt}\" style=\"{}\" loading=\"lazy\">",
        images.region(image, "eq"),
        ctx.unit.css(image.rect.width())
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
    let alt = esc(&figure_alt(block, label));
    let img = image
        .as_ref()
        .filter(|_| block.table.is_none())
        .map(|spec| (images.region(spec, cls), spec))
        .map(|(src, spec)| {
            format!(
                "<img src=\"{src}\" alt=\"{alt}\" style=\"{}\" loading=\"lazy\">",
                ctx.unit.css(spec.rect.width())
            )
        })
        .unwrap_or_default();
    // A reconstructed table replaces the picture of the table.
    let img = match &block.table {
        Some(table) => format!("<div class=\"tbl-wrap\">{}</div>", table_html(table)),
        None => img,
    };
    let cap = if block.spans.is_empty() {
        String::new()
    } else {
        format!("<figcaption>{}</figcaption>", caption_html(&block.spans))
    };
    // Keep the caption on the side it had in the paper.
    let caption_first = match (image, block.lines.first()) {
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
