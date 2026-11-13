//! Hard-95 mode: the page looks like the PDF, but everything that is text is
//! real, visible HTML text.
//!
//! Each page is drawn from the PDF with its text removed (so figures, rules
//! and other graphics stay vector pictures) and the text is put back as HTML
//! at its original place:
//!
//! * paragraphs, headings, captions, footnotes and references are flowing
//!   text in boxes at their original position, initially broken into exactly
//!   the lines of the PDF. A paragraph is one DOM element even when it
//!   continues in another column or on another page; the continuation boxes
//!   show a clipped copy;
//! * tables are real `<table>` elements laid out on the original grid;
//! * formulas are runs of text placed glyph run by glyph run;
//! * everything else (labels in figures, running heads, author lines) is
//!   placed line by line.
//!
//! A small script matches line widths to the PDF and, when the text changes
//! (for example after a translation), lets the affected block reflow inside
//! its box.

use super::*;
use crate::common::geom::median;
use crate::model::Line;
use std::collections::HashSet;

const CSS: &str = include_str!("hard95.css");
const JS: &str = include_str!("hard95.js");

/// Distance of the baseline from the top of a line box whose line-height
/// equals the font size, in em (typical for Times/Helvetica-like fonts).
const BASELINE: f32 = 0.84;

struct Units {
    pw: f32,
    ph: f32,
}

impl Units {
    /// Horizontal position or width as a percentage of the page width.
    fn x(&self, v: f32) -> String {
        format!("{:.3}%", v / self.pw * 100.0)
    }
    /// Vertical position or height as a percentage of the page height.
    fn y(&self, v: f32) -> String {
        format!("{:.3}%", v / self.ph * 100.0)
    }
    /// A length that scales with the page (font sizes, margins).
    fn len(&self, v: f32) -> String {
        format!("{:.3}cqw", v / self.pw * 100.0)
    }
    fn width_attr(&self, v: f32) -> String {
        format!("{:.3}", v / self.pw * 100.0)
    }
}

/// Widths (in em) of the glyphs of a line that have no Unicode value. Such
/// glyphs stay in the picture of the page; the text leaves a gap for them.
fn gaps_of(line: &Line) -> Vec<f32> {
    line.glyphs
        .iter()
        .filter(|g| g.text.contains('\u{FFFD}'))
        .map(|g| ((g.x1 - g.x0) / line.size.max(1.0)).clamp(0.1, 3.0))
        .collect()
}

fn span_html(s: &Span, text: &str, gaps: &mut std::slice::Iter<'_, f32>) -> String {
    let mut classes: Vec<&str> = Vec::new();
    if s.style & MONO != 0 {
        classes.push("m");
    } else if s.style & SANS != 0 {
        classes.push("s");
    }
    if s.style & MATH != 0 {
        classes.push("x");
    }
    let mut out = String::new();
    for (i, piece) in text.split('\u{FFFD}').enumerate() {
        if i > 0 {
            let w = gaps.next().copied().unwrap_or(0.5);
            out.push_str(&format!("<span class=\"ph\" style=\"width:{w:.3}em\"></span>"));
        }
        out.push_str(&esc(piece));
    }
    if s.style & SUP != 0 {
        out = format!("<sup>{out}</sup>");
    }
    if s.style & SUB != 0 {
        out = format!("<sub>{out}</sub>");
    }
    if s.style & ITALIC != 0 {
        out = format!("<i>{out}</i>");
    }
    if s.style & BOLD != 0 {
        out = format!("<b>{out}</b>");
    }
    let color = (s.color != [0; 3]).then(|| format!("color:#{:02x}{:02x}{:02x}", s.color[0], s.color[1], s.color[2]));
    if !classes.is_empty() || color.is_some() {
        let cls = if classes.is_empty() {
            String::new()
        } else {
            format!(" class=\"{}\"", classes.join(" "))
        };
        let style = color.map(|c| format!(" style=\"{c}\"")).unwrap_or_default();
        out = format!("<span{cls}{style}>{out}</span>");
    }
    match &s.href {
        Some(h) if h.starts_with('#') => format!("<a href=\"{}\">{out}</a>", esc(h)),
        Some(h) => format!(
            "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{out}</a>",
            esc(h)
        ),
        None => out,
    }
}

/// Styled spans as HTML. Returns the markup and whether the text ended with
/// a space (which the caller places outside the line element).
fn spans_h95(spans: &[Span], gaps: &[f32]) -> (String, bool) {
    let mut out = String::new();
    let mut trailing = false;
    let mut gaps = gaps.iter();
    for (i, s) in spans.iter().enumerate() {
        let mut text = s.text.as_str();
        if i + 1 == spans.len() && text.ends_with(' ') {
            text = text.trim_end_matches(' ');
            trailing = true;
        }
        if !text.is_empty() {
            out.push_str(&span_html(s, text, &mut gaps));
        }
    }
    (out, trailing)
}

/// A line placed on its own: `left`/`top` position, font size, and the width
/// it must have.
fn placed_line(u: &Units, line: &Line, spans: &[Span]) -> String {
    let (html, _) = spans_h95(spans, &gaps_of(line));
    if html.is_empty() {
        return String::new();
    }
    format!(
        "<span class=\"pl\" style=\"left:{};top:{};font-size:{}\" data-w=\"{}\">{html}</span>",
        u.x(line.bbox.x0),
        u.y(line.baseline - BASELINE * line.size),
        u.len(line.size),
        u.width_attr(line.bbox.width()),
    )
}

/// Consecutive lines of a block that sit in one column of one page.
struct Fragment {
    start: usize,
    end: usize,
    page: usize,
    x0: f32,
    x1: f32,
}

fn fragments(lines: &[Line]) -> Vec<Fragment> {
    let mut out: Vec<Fragment> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let new = match (out.last(), i.checked_sub(1).map(|p| &lines[p])) {
            (Some(f), Some(prev)) => l.page != f.page || l.baseline < prev.baseline - 0.5 * l.size,
            _ => true,
        };
        if new {
            out.push(Fragment {
                start: i,
                end: i + 1,
                page: l.page,
                x0: l.bbox.x0,
                x1: l.bbox.x1,
            });
        } else {
            let f = out.last_mut().unwrap();
            f.end = i + 1;
            f.x0 = f.x0.min(l.bbox.x0);
            f.x1 = f.x1.max(l.bbox.x1);
        }
    }
    out
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

/// The boxes of a flowing block on page `pi`: the block itself where it
/// starts, clipped copies where it continues.
fn flow_block(doc: &Document, b: &Block, pi: usize, u: &Units) -> String {
    if b.lines.is_empty() || b.hlines.len() != b.lines.len() {
        return String::new();
    }
    let frags = fragments(&b.lines);
    if !frags.iter().any(|f| f.page == pi) {
        return String::new();
    }
    let size = median(&b.lines.iter().map(|l| l.size).collect::<Vec<_>>(), doc.body_size);
    // Line pitch of the block.
    let mut pitches = Vec::new();
    for f in &frags {
        for w in b.lines[f.start..f.end].windows(2) {
            let d = w[1].baseline - w[0].baseline;
            if d > 0.5 * size {
                pitches.push(d);
            }
        }
    }
    let lead = median(&pitches, doc.leading * size / doc.body_size).max(0.9 * size);
    let width = frags.iter().map(|f| f.x1 - f.x0).fold(0.0, f32::max);

    // The lines, each kept on a line of its own.
    let mut inner = String::new();
    for (fi, f) in frags.iter().enumerate() {
        for i in f.start..f.end {
            let (l, h) = (&b.lines[i], &b.hlines[i]);
            let (html, trailing) = spans_h95(&h.spans, &gaps_of(l));
            let mut style = format!("margin-left:{}", u.len((l.bbox.x0 - f.x0).max(0.0)));
            if (l.size - size).abs() > 0.3 {
                style.push_str(&format!(";font-size:{}", u.len(l.size)));
            }
            let last = fi + 1 == frags.len() && i + 1 == f.end;
            inner.push_str(&format!(
                "<span class=\"ln{}{}\" style=\"{style}\" data-w=\"{}\">{html}</span>{}",
                if h.soft_hyphen { " hy" } else { "" },
                if last { " end" } else { "" },
                u.width_attr(l.bbox.width()),
                if trailing { " " } else { "" },
            ));
        }
    }

    // How the block is set when it has to reflow.
    let first = &b.lines[0];
    let rest_x0 = b.lines[frags[0].start + 1..frags[0].end]
        .iter()
        .map(|l| l.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    let indent = if rest_x0.is_finite() {
        first.bbox.x0 - rest_x0
    } else {
        0.0
    };
    let full = b
        .lines
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            let f = frags.iter().find(|f| f.start <= *i && *i < f.end).unwrap();
            (f.x1 - l.bbox.x1).abs() < 1.5
        })
        .count();
    let centred = b.lines.len() >= 2 && {
        let cx: Vec<f32> = b.lines.iter().map(|l| l.bbox.cx()).collect();
        let m = median(&cx, 0.0);
        cx.iter().all(|c| (c - m).abs() < 2.5) && b.lines.iter().any(|l| (l.bbox.x0 - frags[0].x0).abs() > 3.0)
    };
    let align = if centred {
        "center"
    } else if b.lines.len() >= 3 && full * 10 >= (b.lines.len() - 1) * 6 {
        "justify"
    } else {
        "left"
    };

    let mut out = String::new();
    let mut lines_before = 0usize;
    for (fi, f) in frags.iter().enumerate() {
        let n = f.end - f.start;
        if f.page == pi {
            let top = b.lines[f.start].baseline - (lead * 0.5 + (BASELINE - 0.5) * size);
            let geometry = format!(
                "left:{};top:{};width:{};height:{};font-size:{};line-height:{};--ti:{};--pl:{}",
                u.x(f.x0),
                u.y(top),
                u.x(width),
                u.len(n as f32 * lead),
                u.len(size),
                u.len(lead),
                u.len(if centred { 0.0 } else { indent }),
                u.len(if centred { 0.0 } else { (-indent).max(0.0) }),
            );
            if fi == 0 {
                out.push_str(&format!(
                    "<div class=\"fb{}\" data-kind=\"{}\" data-block=\"{}\" data-align=\"{align}\" data-parts=\"{}\" style=\"{geometry}\"><div class=\"in\">{inner}</div></div>\n",
                    if frags.len() == 1 { " one" } else { "" },
                    kind_name(&b.kind),
                    b.id,
                    frags.len(),
                ));
            } else {
                // The same text, shifted up by the lines shown before.
                out.push_str(&format!(
                    "<div class=\"fb cont{}\" data-of=\"{}\" data-skip=\"{lines_before}\" data-align=\"{align}\" translate=\"no\" aria-hidden=\"true\" style=\"{geometry}\"><div class=\"in\" style=\"margin-top:-{}\">{inner}</div></div>\n",
                    if fi + 1 == frags.len() { " tail" } else { "" },
                    b.id,
                    u.len(lines_before as f32 * lead),
                ));
            }
        }
        lines_before += n;
    }
    out
}

/// A reconstructed table laid out on its original grid.
fn table_box(t: &Table, u: &Units) -> String {
    let g = &t.geom;
    let (bw, bh) = (g.bbox.width().max(1.0), g.bbox.height().max(1.0));
    let mut out = format!(
        "<div class=\"tb\" data-kind=\"table\" style=\"left:{};top:{};width:{};height:{};font-size:{}\"><table><colgroup>",
        u.x(g.bbox.x0),
        u.y(g.bbox.y0),
        u.x(bw),
        u.y(bh),
        u.len(g.size),
    );
    for c in 0..t.n_cols {
        out.push_str(&format!(
            "<col style=\"width:{:.3}%\">",
            (g.col_bounds[c + 1] - g.col_bounds[c]) / bw * 100.0
        ));
    }
    out.push_str("</colgroup>");
    let mut open_row: Option<usize> = None;
    table_walk(t, |ri, col, cell| {
        if open_row != Some(ri) {
            if open_row.is_some() {
                out.push_str("</tr>");
            }
            out.push_str(&format!(
                "<tr style=\"height:{:.3}%\">",
                (g.row_bounds[ri + 1] - g.row_bounds[ri]) / bh * 100.0
            ));
            open_row = Some(ri);
        }
        let tag = if ri < t.header_rows { "th" } else { "td" };
        match cell {
            None => out.push_str(&format!(
                "<{tag} class=\"e{}\"></{tag}>",
                table_rule_classes(t, ri, col, 1, 1)
            )),
            Some(cell) => {
                let c0 = cell.col.min(t.n_cols - 1);
                let c1 = (cell.col + cell.colspan).min(t.n_cols) - 1;
                // Put the first line on the baseline it has in the PDF (cells
                // spanning rows are centred instead).
                let top = if cell.rowspan > 1 {
                    0.0
                } else {
                    (g.row_baselines[ri] - BASELINE * g.size - g.row_bounds[ri]).max(0.0)
                };
                let (align, pad) = if cell.colspan > 1 {
                    ("c", format!(" style=\"padding-top:{}\"", u.len(top)))
                } else {
                    // Keep the text inside the band it occupies in the PDF.
                    let left = (g.col_text[c0].0 - g.col_bounds[c0]).max(0.0);
                    let right = (g.col_bounds[c1 + 1] - g.col_text[c1].1).max(0.0);
                    (
                        &t.col_align[c0][..1],
                        format!(" style=\"padding:{} {} 0 {}\"", u.len(top), u.len(right), u.len(left)),
                    )
                };
                let mut attrs = String::new();
                if cell.colspan > 1 {
                    attrs.push_str(&format!(" colspan=\"{}\"", cell.colspan));
                }
                if cell.rowspan > 1 {
                    attrs.push_str(&format!(" rowspan=\"{}\"", cell.rowspan));
                }
                let (html, _) = spans_h95(&cell.spans, &[]);
                out.push_str(&format!(
                    "<{tag}{attrs} class=\"{align}{}{}\"{pad}>{html}</{tag}>",
                    if cell.rowspan > 1 { " mid" } else { "" },
                    table_rule_classes(t, ri, cell.col, cell.colspan, cell.rowspan),
                ));
            }
        }
    });
    if open_row.is_some() {
        out.push_str("</tr>");
    }
    out.push_str("</table></div>\n");
    out
}

/// The glyph runs of a displayed formula.
fn equation(b: &Block, u: &Units) -> String {
    let mut out = format!(
        "<div class=\"eq\" data-kind=\"equation\" data-block=\"{}\" translate=\"no\">",
        b.id
    );
    for r in &b.eq_runs {
        let span = Span {
            text: r.text.clone(),
            style: r.style | MATH,
            href: None,
            color: r.color,
        };
        let html = span_html(&span, &r.text, &mut [].iter());
        let ink = r
            .ink
            .map(|i| {
                format!(
                    " data-ink=\"{:.3},{:.3},{:.3},{:.3}\"",
                    i.x0 / u.pw * 100.0,
                    i.y0 / u.ph * 100.0,
                    i.x1 / u.pw * 100.0,
                    i.y1 / u.ph * 100.0
                )
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "<span class=\"g\" style=\"left:{};top:{};font-size:{}\" data-w=\"{}\"{ink}>{html}</span>",
            u.x(r.x),
            u.y(r.y - BASELINE * r.size),
            u.len(r.size),
            u.width_attr(r.width),
        ));
    }
    out.push_str("</div>\n");
    out
}

fn add_origins(set: &mut HashSet<(i32, i32)>, line: &Line) {
    for g in &line.glyphs {
        // Glyphs without a Unicode value cannot be written as text.
        if g.text.contains('\u{FFFD}') {
            continue;
        }
        set.insert(((g.ox * 4.0).round() as i32, (g.oy * 4.0).round() as i32));
        if let Some((x, y)) = g.extra {
            set.insert(((x * 4.0).round() as i32, (y * 4.0).round() as i32));
        }
    }
}

/// The `hard-95` mode: facsimile pages whose text is real, visible HTML.
pub(crate) struct Hard95;

impl Renderer for Hard95 {
    fn render(&self, analysis: &Analysis, images: &mut ImageStore<'_>) -> String {
        render(analysis, images)
    }
}

pub(crate) fn render(analysis: &Analysis, images: &mut ImageStore<'_>) -> String {
    let doc = &analysis.doc;
    let title = doc_title(doc);
    let lower = doc.body_font.to_ascii_lowercase();
    let computer_modern = ["cmr", "lmr", "lmroman", "cmu", "sfrm", "ecrm"]
        .iter()
        .any(|p| lower.contains(p));

    let mut pages = String::new();
    for (pi, p) in doc.pages.iter().enumerate() {
        let u = Units {
            pw: p.width,
            ph: p.height,
        };
        let mut layer = String::new();
        // Origins of all glyphs that are drawn as HTML on this page; they
        // are removed from the picture of the page.
        let mut drawn: HashSet<(i32, i32)> = HashSet::new();

        for b in &doc.blocks {
            match &b.kind {
                BlockKind::Equation { .. } => {
                    if b.lines.first().is_some_and(|l| l.page == pi) {
                        layer.push_str(&equation(b, &u));
                        b.lines.iter().for_each(|l| add_origins(&mut drawn, l));
                    }
                }
                BlockKind::Authors => {
                    let mut html = String::new();
                    for (l, h) in b.lines.iter().zip(&b.hlines).filter(|(l, _)| l.page == pi) {
                        html.push_str(&placed_line(&u, l, &h.spans));
                        add_origins(&mut drawn, l);
                    }
                    if !html.is_empty() {
                        layer.push_str(&format!(
                            "<div class=\"b\" data-kind=\"authors\" data-block=\"{}\">{html}</div>\n",
                            b.id
                        ));
                    }
                }
                _ => {
                    // Caption or text block.
                    let html = flow_block(doc, b, pi, &u);
                    if !html.is_empty() {
                        layer.push_str(&html);
                        b.lines
                            .iter()
                            .filter(|l| l.page == pi)
                            .for_each(|l| add_origins(&mut drawn, l));
                    }
                    // Content of a float: a table, or text placed line by line.
                    if b.inner_lines.first().is_some_and(|l| l.page == pi) {
                        match &b.table {
                            Some(t) => layer.push_str(&table_box(t, &u)),
                            None => {
                                let mut html = String::new();
                                for (l, spans) in b.inner_lines.iter().zip(&b.inner_spans) {
                                    html.push_str(&placed_line(&u, l, spans));
                                }
                                layer.push_str(&format!(
                                    "<div class=\"b\" data-kind=\"figure-text\" data-block=\"{}\">{html}</div>\n",
                                    b.id
                                ));
                            }
                        }
                        b.inner_lines.iter().for_each(|l| add_origins(&mut drawn, l));
                    }
                }
            }
        }
        if let Some(loose) = analysis.loose.get(pi) {
            let mut html = String::new();
            for l in loose {
                html.push_str(&placed_line(&u, &l.line, &l.spans));
                add_origins(&mut drawn, &l.line);
            }
            if !html.is_empty() {
                layer.push_str(&format!("<div class=\"b\" data-kind=\"page-furniture\">{html}</div>\n"));
            }
        }

        // Rotated text, turned around the start of its baseline.
        if let Some(rotated) = analysis.rotated.get(pi) {
            let mut html = String::new();
            for r in rotated {
                let span = Span {
                    text: r.text.clone(),
                    style: r.style,
                    href: None,
                    color: r.color,
                };
                html.push_str(&format!(
                    "<span class=\"rt\" style=\"left:{};top:{};font-size:{};transform:rotate({:.2}deg)\" data-w=\"{}\">{}</span>",
                    u.x(r.x),
                    u.y(r.y - BASELINE * r.size),
                    u.len(r.size),
                    r.angle,
                    u.width_attr(r.length),
                    span_html(&span, &r.text, &mut [].iter()),
                ));
                for &(x, y) in &r.origins {
                    drawn.insert(((x * 4.0).round() as i32, (y * 4.0).round() as i32));
                }
            }
            if !html.is_empty() {
                layer.push_str(&format!("<div class=\"b\" data-kind=\"rotated-text\">{html}</div>\n"));
            }
        }

        let bg = format!(
            "<img class=\"bg\" src=\"{}\" alt=\"\" width=\"{}\" height=\"{}\" loading=\"lazy\" decoding=\"async\">",
            images.page_without_text(pi, drawn),
            p.width,
            p.height
        );
        pages.push_str(&format!(
            "<section class=\"page\" id=\"page-{n}\" data-page=\"{n}\" style=\"--pw:{};--ph:{};aspect-ratio:{}/{}\">\n{bg}\n<div class=\"tl\">\n{layer}</div>\n</section>\n",
            p.width,
            p.height,
            p.width,
            p.height,
            n = pi + 1,
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
<meta name="generator" content="paper2html (hard-95)">
<title>{title}</title>
<style>
{CSS}</style>
</head>
<body class="hard95 fit{cm}">
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
        cm = if computer_modern { " cm" } else { "" },
    )
}
