//! Flowing HTML for a sequence of blocks, shared by the medium and friendly modes.

use super::*;
use crate::model::*;

/// How image widths are expressed in a given mode.
pub(crate) enum WidthUnit {
    /// Relative to the body font size (`pt_per_em` points per em).
    Em { pt_per_em: f32 },
    /// Relative to the page container width (`page_width` points wide).
    Cqw { page_width: f32 },
}

impl WidthUnit {
    fn css(&self, width_pt: f32) -> String {
        match self {
            WidthUnit::Em { pt_per_em } => format!("width:min(100%,{:.2}em)", width_pt / pt_per_em),
            WidthUnit::Cqw { page_width } => format!("width:min(100%,{:.2}cqw)", width_pt / page_width * 100.0),
        }
    }
}

pub(crate) struct FlowCtx {
    pub unit: WidthUnit,
}

pub(crate) fn heading_tag(level: u8) -> &'static str {
    match level {
        0 | 1 => "h2",
        2 => "h3",
        3 => "h4",
        _ => "h5",
    }
}

/// Renders a sequence of blocks as flowing HTML, grouping list items,
/// references and abstract paragraphs.
pub(crate) fn render_flow(blocks: &[&Block], images: &mut ImageStore<'_>, ctx: &FlowCtx) -> String {
    #[derive(PartialEq, Clone, Copy)]
    enum Group {
        None,
        List,
        Refs,
        Abstract,
    }
    let mut out = String::new();
    let mut group = Group::None;
    let close = |g: Group, out: &mut String| match g {
        Group::List => out.push_str("</ul>\n"),
        Group::Refs => out.push_str("</ol>\n"),
        Group::Abstract => out.push_str("</section>\n"),
        Group::None => {}
    };
    for (bi, b) in blocks.iter().enumerate() {
        let want = match &b.kind {
            BlockKind::ListItem { .. } => Group::List,
            BlockKind::Reference { .. } => Group::Refs,
            BlockKind::Paragraph { in_abstract: true } => Group::Abstract,
            // Floats, equations and footnotes do not interrupt a group.
            BlockKind::Equation { .. } if group == Group::Abstract => group,
            _ => Group::None,
        };
        if want != group {
            close(group, &mut out);
            match want {
                Group::List => out.push_str("<ul class=\"list\">\n"),
                Group::Refs => out.push_str("<ol class=\"refs\">\n"),
                Group::Abstract => {
                    // The "Abstract" heading (if any) was emitted just before.
                    let had_heading = bi > 0
                        && matches!(blocks[bi - 1].kind, BlockKind::Heading { .. })
                        && plain(&blocks[bi - 1].spans)
                            .trim_end_matches(['.', ':'])
                            .eq_ignore_ascii_case("abstract");
                    out.push_str("<section class=\"abstract\">\n");
                    if !had_heading {
                        out.push_str("<div class=\"abstract-label\">Abstract</div>\n");
                    }
                }
                Group::None => {}
            }
            group = want;
        }
        let id = id_attr(b);
        match &b.kind {
            BlockKind::Title | BlockKind::Authors => {}
            BlockKind::Note => out.push_str(&format!("<p class=\"note\">{}</p>\n", spans_html(&b.spans))),
            BlockKind::Heading { level } => {
                let tag = heading_tag(*level);
                let is_abstract = plain(&b.spans)
                    .trim_end_matches(['.', ':'])
                    .eq_ignore_ascii_case("abstract");
                let cls = if is_abstract { " class=\"abstract-heading\"" } else { "" };
                out.push_str(&format!("<{tag}{id}{cls}>{}</{tag}>\n", spans_html(&b.spans)));
            }
            BlockKind::Paragraph { .. } => out.push_str(&format!("<p{id}>{}</p>\n", spans_html(&b.spans))),
            BlockKind::ListItem { marker } => {
                let cls = if marker.is_some() { " class=\"labeled\"" } else { "" };
                out.push_str(&format!("<li{id}{cls}>{}</li>\n", spans_html(&b.spans)));
            }
            BlockKind::Reference { .. } => out.push_str(&format!("<li{id}>{}</li>\n", spans_html(&b.spans))),
            BlockKind::Footnote { marker } => {
                let mark = if marker.is_empty() {
                    String::new()
                } else {
                    format!("<sup class=\"fn-mark\">{}</sup> ", esc(marker))
                };
                out.push_str(&format!(
                    "<aside class=\"footnote\"{id} role=\"note\">{mark}{}</aside>\n",
                    spans_html(&b.spans)
                ));
            }
            BlockKind::Equation { image, number } => {
                let alt = esc(&plain(&b.spans));
                let img = format!(
                    "<img src=\"{}\" alt=\"{alt}\" style=\"{}\" loading=\"lazy\">",
                    images.region(image, "eq"),
                    ctx.unit.css(image.rect.width())
                );
                let num = number
                    .as_ref()
                    .map(|n| format!("<span class=\"eqno\">{}</span>", esc(n)))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "<div class=\"eq\"{id}><span class=\"eq-body\">{img}</span>{num}</div>\n"
                ));
            }
            BlockKind::Figure { kind, image, label } => {
                let cls = float_class(*kind);
                let alt = esc(&figure_alt(b, label));
                let img = image
                    .as_ref()
                    .filter(|_| b.table.is_none())
                    .map(|spec| (images.region(spec, cls), spec))
                    .map(|(src, spec)| {
                        format!(
                            "<img src=\"{src}\" alt=\"{alt}\" style=\"{}\" loading=\"lazy\">",
                            ctx.unit.css(spec.rect.width())
                        )
                    })
                    .unwrap_or_default();
                // A reconstructed table replaces the picture of the table.
                let img = match &b.table {
                    Some(t) => format!("<div class=\"tbl-wrap\">{}</div>", table_html(t)),
                    None => img,
                };
                let cap = if b.spans.is_empty() {
                    String::new()
                } else {
                    format!("<figcaption>{}</figcaption>", caption_html(&b.spans))
                };
                // Keep the caption on the side it had in the paper.
                let caption_first = match (image, b.lines.first()) {
                    (Some(spec), Some(l)) => l.bbox.cy() < spec.rect.cy(),
                    _ => false,
                };
                let inner = if caption_first {
                    format!("{cap}{img}")
                } else {
                    format!("{img}{cap}")
                };
                out.push_str(&format!("<figure class=\"float {cls}\"{id}>{inner}</figure>\n"));
            }
        }
    }
    close(group, &mut out);
    out
}

/// Author groups as cards.
pub(crate) fn authors_html(blocks: &[&Block]) -> String {
    let cards: Vec<String> = blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Authors))
        .map(|b| {
            let rows: Vec<String> = b
                .rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    format!(
                        "<div class=\"{}\">{}</div>",
                        if i == 0 { "au-name" } else { "au-line" },
                        spans_html(r)
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
