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
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use super::assets::origin_key;
use super::*;
use crate::common::geom::median;
use crate::model::Line;
use std::collections::HashSet;

/// Style sheet of the mode, embedded in the page.
const CSS: &str = include_str!("hard95.css");
/// Script of the mode (zoom, line widths, reflow), embedded in the page.
const JS: &str = include_str!("hard95.js");

/// Distance of the baseline from the top of a line box whose line-height
/// equals the font size, in em (typical for Times/Helvetica-like fonts).
const BASELINE: f32 = 0.84;

/// Prefixes of the (lower-cased) body font names of Computer Modern and its
/// relatives; such papers get the `cm` class, which selects a matching web
/// font.
const COMPUTER_MODERN_PREFIXES: [&str; 6] = ["cmr", "lmr", "lmroman", "cmu", "sfrm", "ecrm"];

/// Smallest and largest width of the gap left for a glyph without Unicode
/// value, in em: keeps odd glyph boxes from making absurd gaps.
const GAP_WIDTH_RANGE_EM: (f32, f32) = (0.1, 3.0);
/// Width of a gap for which no glyph width is known, in em.
const DEFAULT_GAP_EM: f32 = 0.5;

/// A line whose baseline is higher than the previous one by more than this
/// (in em of its font size) starts a new fragment: the block continues at
/// the top of another column.
const COLUMN_BREAK_RISE_EM: f32 = 0.5;
/// Distances between consecutive baselines below this (in em of the block
/// font size) are not line pitches (superscript lines, overlapping lines).
const MIN_PITCH_EM: f32 = 0.5;
/// Smallest line pitch of a block, in em of its font size.
const MIN_LEAD_EM: f32 = 0.9;
/// A line whose font size differs from the block font size by more than
/// this (in points) gets its own font size.
const FONT_SIZE_TOLERANCE_PT: f32 = 0.3;
/// A line whose right end is within this distance (in points) of the right
/// edge of its fragment is a full line.
const FULL_LINE_TOLERANCE_PT: f32 = 1.5;
/// Lines whose centres are all within this distance (in points) of their
/// median centre are centred...
const CENTRED_TOLERANCE_PT: f32 = 2.5;
/// ...provided at least one line starts more than this (in points) from the
/// left edge of the block (otherwise the block is simply left-aligned).
const CENTRED_MIN_OFFSET_PT: f32 = 3.0;
/// Fewest lines a block needs to be recognised as centred.
const CENTRED_MIN_LINES: usize = 2;
/// Fewest lines a block needs to be recognised as justified.
const JUSTIFY_MIN_LINES: usize = 3;
/// A block is justified when at least this many tenths of its lines (not
/// counting the last one) are full lines.
const JUSTIFY_FULL_TENTHS: usize = 6;

/// Converts PDF points to CSS lengths relative to one page.
struct Units {
    /// Width of the page in points.
    page_width: f32,
    /// Height of the page in points.
    page_height: f32,
}

impl Units {
    /// Horizontal position or width as a percentage of the page width.
    fn x(&self, value: f32) -> String {
        format!("{:.3}%", value / self.page_width * 100.0)
    }
    /// Vertical position or height as a percentage of the page height.
    fn y(&self, value: f32) -> String {
        format!("{:.3}%", value / self.page_height * 100.0)
    }
    /// A length that scales with the page (font sizes, margins).
    fn len(&self, value: f32) -> String {
        format!("{:.3}cqw", value / self.page_width * 100.0)
    }
    /// A width as a percentage of the page width, without unit (for the
    /// `data-w` attributes read by the script).
    fn width_attr(&self, value: f32) -> String {
        format!("{:.3}", value / self.page_width * 100.0)
    }
}

/// Consecutive lines of a block that sit in one column of one page.
struct Fragment {
    /// Index of the first line in the block.
    start: usize,
    /// Index after the last line in the block.
    end: usize,
    /// Page of the lines.
    page: usize,
    /// Left edge of the lines.
    x0: f32,
    /// Right edge of the lines.
    x1: f32,
}

/// How the boxes of a flowing block are set: shared by all its fragments.
struct BoxStyle {
    /// Font size of the block (median of its lines).
    size: f32,
    /// Line pitch of the block.
    lead: f32,
    /// Width of the widest fragment.
    width: f32,
    /// Indent of the first line relative to the others (negative for a
    /// hanging indent).
    indent: f32,
    /// The lines are centred.
    centred: bool,
    /// Text alignment when the block reflows: `center`, `justify` or `left`.
    align: &'static str,
}

/// The text layer of one page being written, with the glyphs it draws.
struct PageLayer<'u> {
    /// Units of the page.
    units: &'u Units,
    /// Index of the page.
    page_idx: usize,
    /// The HTML of the layer.
    html: String,
    /// Origins of all glyphs that are drawn as HTML on this page; they are
    /// removed from the picture of the page.
    drawn: HashSet<(i32, i32)>,
}

impl PageLayer<'_> {
    /// Adds the part of `block` that is on this page: an equation, author
    /// lines, or a text block with the content of its float.
    fn block(&mut self, doc: &Document, block: &Block) {
        match &block.kind {
            BlockKind::Equation { .. } => self.equation(block),
            BlockKind::Authors => self.authors(block),
            _ => {
                // Caption or text block.
                self.text_block(doc, block);
                self.float_content(block);
            }
        }
    }

    /// Adds an equation that starts on this page, glyph run by glyph run.
    fn equation(&mut self, block: &Block) {
        if block.lines.first().is_some_and(|line| line.page == self.page_idx) {
            self.html.push_str(&equation(block, self.units));
            block.lines.iter().for_each(|line| add_origins(&mut self.drawn, line));
        }
    }

    /// Adds the author lines on this page, each placed on its own.
    fn authors(&mut self, block: &Block) {
        let mut html = String::new();
        for (line, hline) in block
            .lines
            .iter()
            .zip(&block.hlines)
            .filter(|(line, _)| line.page == self.page_idx)
        {
            html.push_str(&placed_line(self.units, line, &hline.spans));
            add_origins(&mut self.drawn, line);
        }
        if !html.is_empty() {
            self.html.push_str(&format!(
                "<div class=\"b\" data-kind=\"authors\" data-block=\"{}\">{html}</div>\n",
                block.id
            ));
        }
    }

    /// Adds the boxes of a flowing text block (or caption) on this page.
    fn text_block(&mut self, doc: &Document, block: &Block) {
        let html = flow_block(doc, block, self.page_idx, self.units);
        if !html.is_empty() {
            self.html.push_str(&html);
            block
                .lines
                .iter()
                .filter(|line| line.page == self.page_idx)
                .for_each(|line| add_origins(&mut self.drawn, line));
        }
    }

    /// Adds the content of a float that starts on this page: a table, or
    /// its text placed line by line.
    fn float_content(&mut self, block: &Block) {
        if !block.inner_lines.first().is_some_and(|line| line.page == self.page_idx) {
            return;
        }
        match &block.table {
            Some(table) => self.html.push_str(&table_box(table, self.units)),
            None => {
                let mut html = String::new();
                for (line, spans) in block.inner_lines.iter().zip(&block.inner_spans) {
                    html.push_str(&placed_line(self.units, line, spans));
                }
                self.html.push_str(&format!(
                    "<div class=\"b\" data-kind=\"figure-text\" data-block=\"{}\">{html}</div>\n",
                    block.id
                ));
            }
        }
        block
            .inner_lines
            .iter()
            .for_each(|line| add_origins(&mut self.drawn, line));
    }

    /// Adds the lines of the page that belong to no block (running heads,
    /// page numbers), each placed on its own.
    fn furniture(&mut self, loose: &[PLine]) {
        let mut html = String::new();
        for loose_line in loose {
            html.push_str(&placed_line(self.units, &loose_line.line, &loose_line.spans));
            add_origins(&mut self.drawn, &loose_line.line);
        }
        if !html.is_empty() {
            self.html
                .push_str(&format!("<div class=\"b\" data-kind=\"page-furniture\">{html}</div>\n"));
        }
    }

    /// Adds rotated text, turned around the start of its baseline.
    fn rotated(&mut self, rotated: &[RotatedRun]) {
        let units = self.units;
        let mut html = String::new();
        for run in rotated {
            let span = Span {
                text: run.text.clone(),
                style: run.style,
                href: None,
                color: run.color,
            };
            html.push_str(&format!(
                "<span class=\"rt\" style=\"left:{};top:{};font-size:{};transform:rotate({:.2}deg)\" data-w=\"{}\">{}</span>",
                units.x(run.x),
                units.y(run.y - BASELINE * run.size),
                units.len(run.size),
                run.angle,
                units.width_attr(run.length),
                span_html(&span, &run.text, &mut [].iter()),
            ));
            for &(x, y) in &run.origins {
                self.drawn.insert(origin_key(x, y));
            }
        }
        if !html.is_empty() {
            self.html
                .push_str(&format!("<div class=\"b\" data-kind=\"rotated-text\">{html}</div>\n"));
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

/// Writes the whole document in hard-95 mode. The page pictures (without
/// the text drawn as HTML) are requested from `images`, one per page in page
/// order.
pub(crate) fn render(analysis: &Analysis, images: &mut ImageStore<'_>) -> String {
    let doc = &analysis.doc;
    let title = doc_title(doc);
    let lower = doc.body_font.to_ascii_lowercase();
    let computer_modern = COMPUTER_MODERN_PREFIXES.iter().any(|prefix| lower.contains(prefix));

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

/// One page: the picture of the page without the text that the text layer
/// over it draws.
fn page_html(analysis: &Analysis, page_idx: usize, page: &PageInfo, images: &mut ImageStore<'_>) -> String {
    let doc = &analysis.doc;
    let units = Units {
        page_width: page.width,
        page_height: page.height,
    };
    let mut layer = PageLayer {
        units: &units,
        page_idx,
        html: String::new(),
        drawn: HashSet::new(),
    };
    for block in &doc.blocks {
        layer.block(doc, block);
    }
    if let Some(loose) = analysis.loose.get(page_idx) {
        layer.furniture(loose);
    }
    if let Some(rotated) = analysis.rotated.get(page_idx) {
        layer.rotated(rotated);
    }
    let PageLayer { html: layer, drawn, .. } = layer;

    let bg = format!(
        "<img class=\"bg\" src=\"{}\" alt=\"\" width=\"{}\" height=\"{}\" loading=\"lazy\" decoding=\"async\">",
        images.page_without_text(page_idx, drawn),
        page.width,
        page.height
    );
    format!(
        "<section class=\"page\" id=\"page-{n}\" data-page=\"{n}\" style=\"--pw:{};--ph:{};aspect-ratio:{}/{}\">\n{bg}\n<div class=\"tl\">\n{layer}</div>\n</section>\n",
        page.width,
        page.height,
        page.width,
        page.height,
        n = page_idx + 1,
    )
}

/// Widths (in em) of the glyphs of a line that have no Unicode value. Such
/// glyphs stay in the picture of the page; the text leaves a gap for them.
fn gaps_of(line: &Line) -> Vec<f32> {
    line.glyphs
        .iter()
        .filter(|glyph| glyph.text.contains('\u{FFFD}'))
        .map(|glyph| ((glyph.x1 - glyph.x0) / line.size.max(1.0)).clamp(GAP_WIDTH_RANGE_EM.0, GAP_WIDTH_RANGE_EM.1))
        .collect()
}

/// One styled span as HTML. `text` is the part of the span's text to write;
/// each U+FFFD in it becomes a gap whose width is taken from `gaps`.
fn span_html(span: &Span, text: &str, gaps: &mut std::slice::Iter<'_, f32>) -> String {
    let mut classes: Vec<&str> = Vec::new();
    if span.style & MONO != 0 {
        classes.push("m");
    } else if span.style & SANS != 0 {
        classes.push("s");
    }
    if span.style & MATH != 0 {
        classes.push("x");
    }
    let mut out = String::new();
    for (i, piece) in text.split('\u{FFFD}').enumerate() {
        if i > 0 {
            let width_em = gaps.next().copied().unwrap_or(DEFAULT_GAP_EM);
            out.push_str(&format!("<span class=\"ph\" style=\"width:{width_em:.3}em\"></span>"));
        }
        out.push_str(&esc(piece));
    }
    if span.style & SUP != 0 {
        out = format!("<sup>{out}</sup>");
    }
    if span.style & SUB != 0 {
        out = format!("<sub>{out}</sub>");
    }
    if span.style & ITALIC != 0 {
        out = format!("<i>{out}</i>");
    }
    if span.style & BOLD != 0 {
        out = format!("<b>{out}</b>");
    }
    let color = (span.color != [0; 3])
        .then(|| format!("color:#{:02x}{:02x}{:02x}", span.color[0], span.color[1], span.color[2]));
    if !classes.is_empty() || color.is_some() {
        let cls = if classes.is_empty() {
            String::new()
        } else {
            format!(" class=\"{}\"", classes.join(" "))
        };
        let style = color.map(|color| format!(" style=\"{color}\"")).unwrap_or_default();
        out = format!("<span{cls}{style}>{out}</span>");
    }
    match &span.href {
        Some(href) if href.starts_with('#') => format!("<a href=\"{}\">{out}</a>", esc(href)),
        Some(href) => format!(
            "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{out}</a>",
            esc(href)
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
    for (i, span) in spans.iter().enumerate() {
        let mut text = span.text.as_str();
        if i + 1 == spans.len() && text.ends_with(' ') {
            text = text.trim_end_matches(' ');
            trailing = true;
        }
        if !text.is_empty() {
            out.push_str(&span_html(span, text, &mut gaps));
        }
    }
    (out, trailing)
}

/// A line placed on its own: `left`/`top` position, font size, and the width
/// it must have. Returns an empty string when the line has no text.
fn placed_line(units: &Units, line: &Line, spans: &[Span]) -> String {
    let (html, _) = spans_h95(spans, &gaps_of(line));
    if html.is_empty() {
        return String::new();
    }
    format!(
        "<span class=\"pl\" style=\"left:{};top:{};font-size:{}\" data-w=\"{}\">{html}</span>",
        units.x(line.bbox.x0),
        units.y(line.baseline - BASELINE * line.size),
        units.len(line.size),
        units.width_attr(line.bbox.width()),
    )
}

/// Splits the lines of a block into fragments: a new fragment starts on a
/// new page and where the baseline goes up (a new column).
fn fragments(lines: &[Line]) -> Vec<Fragment> {
    let mut out: Vec<Fragment> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let new = match (out.last(), i.checked_sub(1).map(|prev_idx| &lines[prev_idx])) {
            (Some(frag), Some(prev)) => {
                line.page != frag.page || line.baseline < prev.baseline - COLUMN_BREAK_RISE_EM * line.size
            }
            _ => true,
        };
        if new {
            out.push(Fragment {
                start: i,
                end: i + 1,
                page: line.page,
                x0: line.bbox.x0,
                x1: line.bbox.x1,
            });
        } else {
            let frag = out.last_mut().unwrap();
            frag.end = i + 1;
            frag.x0 = frag.x0.min(line.bbox.x0);
            frag.x1 = frag.x1.max(line.bbox.x1);
        }
    }
    out
}

/// The boxes of a flowing block on page `page_idx`: the block itself where it
/// starts, clipped copies where it continues. Returns an empty string when
/// the block has no line on the page, or no line text to show.
fn flow_block(doc: &Document, block: &Block, page_idx: usize, units: &Units) -> String {
    if block.lines.is_empty() || block.hlines.len() != block.lines.len() {
        return String::new();
    }
    let frags = fragments(&block.lines);
    if !frags.iter().any(|frag| frag.page == page_idx) {
        return String::new();
    }
    let size = median(
        &block.lines.iter().map(|line| line.size).collect::<Vec<_>>(),
        doc.body_size,
    );
    let lead = line_pitch(doc, block, &frags, size);
    let width = frags.iter().map(|frag| frag.x1 - frag.x0).fold(0.0, f32::max);
    let inner = flow_lines(block, &frags, size, units);
    let style = box_style(block, &frags, size, lead, width);
    fragment_boxes(block, &frags, page_idx, &style, &inner, units)
}

/// Line pitch of a block of font size `size`: the median distance between
/// consecutive baselines within its fragments, or the document leading
/// scaled to the block font size when it has no such pair.
fn line_pitch(doc: &Document, block: &Block, frags: &[Fragment], size: f32) -> f32 {
    let mut pitches = Vec::new();
    for frag in frags {
        for pair in block.lines[frag.start..frag.end].windows(2) {
            let pitch = pair[1].baseline - pair[0].baseline;
            if pitch > MIN_PITCH_EM * size {
                pitches.push(pitch);
            }
        }
    }
    median(&pitches, doc.leading * size / doc.body_size).max(MIN_LEAD_EM * size)
}

/// The lines of a flowing block, each kept on a line of its own, with their
/// indent within the fragment and the width they must have.
fn flow_lines(block: &Block, frags: &[Fragment], size: f32, units: &Units) -> String {
    let mut inner = String::new();
    for (frag_idx, frag) in frags.iter().enumerate() {
        for i in frag.start..frag.end {
            let (line, hline) = (&block.lines[i], &block.hlines[i]);
            let (html, trailing) = spans_h95(&hline.spans, &gaps_of(line));
            let mut style = format!("margin-left:{}", units.len((line.bbox.x0 - frag.x0).max(0.0)));
            if (line.size - size).abs() > FONT_SIZE_TOLERANCE_PT {
                style.push_str(&format!(";font-size:{}", units.len(line.size)));
            }
            let last = frag_idx + 1 == frags.len() && i + 1 == frag.end;
            inner.push_str(&format!(
                "<span class=\"ln{}{}\" style=\"{style}\" data-w=\"{}\">{html}</span>{}",
                if hline.soft_hyphen { " hy" } else { "" },
                if last { " end" } else { "" },
                units.width_attr(line.bbox.width()),
                if trailing { " " } else { "" },
            ));
        }
    }
    inner
}

/// How the block is set when it has to reflow: first-line indent and
/// alignment (centred, justified or left), from the geometry of its lines.
fn box_style(block: &Block, frags: &[Fragment], size: f32, lead: f32, width: f32) -> BoxStyle {
    let first = &block.lines[0];
    let rest_x0 = block.lines[frags[0].start + 1..frags[0].end]
        .iter()
        .map(|line| line.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    let indent = if rest_x0.is_finite() {
        first.bbox.x0 - rest_x0
    } else {
        0.0
    };
    let full = block
        .lines
        .iter()
        .enumerate()
        .filter(|(i, line)| {
            let frag = frags.iter().find(|frag| frag.start <= *i && *i < frag.end).unwrap();
            (frag.x1 - line.bbox.x1).abs() < FULL_LINE_TOLERANCE_PT
        })
        .count();
    let centred = block.lines.len() >= CENTRED_MIN_LINES && {
        let centers: Vec<f32> = block.lines.iter().map(|line| line.bbox.cx()).collect();
        let mid = median(&centers, 0.0);
        centers.iter().all(|center| (center - mid).abs() < CENTRED_TOLERANCE_PT)
            && block
                .lines
                .iter()
                .any(|line| (line.bbox.x0 - frags[0].x0).abs() > CENTRED_MIN_OFFSET_PT)
    };
    let align = if centred {
        "center"
    } else if block.lines.len() >= JUSTIFY_MIN_LINES && full * 10 >= (block.lines.len() - 1) * JUSTIFY_FULL_TENTHS {
        "justify"
    } else {
        "left"
    };
    BoxStyle {
        size,
        lead,
        width,
        indent,
        centred,
        align,
    }
}

/// The boxes of the fragments of a block that are on page `page_idx`, each
/// showing `inner` (the lines of the whole block). The first fragment holds
/// the text; the others are clipped copies shifted up by the lines shown
/// before them, hidden from assistive technology and translation.
fn fragment_boxes(
    block: &Block,
    frags: &[Fragment],
    page_idx: usize,
    style: &BoxStyle,
    inner: &str,
    units: &Units,
) -> String {
    let BoxStyle {
        size,
        lead,
        width,
        indent,
        centred,
        align,
    } = *style;
    let mut out = String::new();
    let mut lines_before = 0usize;
    for (frag_idx, frag) in frags.iter().enumerate() {
        let n = frag.end - frag.start;
        if frag.page == page_idx {
            let top = block.lines[frag.start].baseline - (lead * 0.5 + (BASELINE - 0.5) * size);
            let geometry = format!(
                "left:{};top:{};width:{};height:{};font-size:{};line-height:{};--ti:{};--pl:{}",
                units.x(frag.x0),
                units.y(top),
                units.x(width),
                units.len(n as f32 * lead),
                units.len(size),
                units.len(lead),
                units.len(if centred { 0.0 } else { indent }),
                units.len(if centred { 0.0 } else { (-indent).max(0.0) }),
            );
            if frag_idx == 0 {
                out.push_str(&format!(
                    "<div class=\"fb{}\" data-kind=\"{}\" data-block=\"{}\" data-align=\"{align}\" data-parts=\"{}\" style=\"{geometry}\"><div class=\"in\">{inner}</div></div>\n",
                    if frags.len() == 1 { " one" } else { "" },
                    kind_name(&block.kind),
                    block.id,
                    frags.len(),
                ));
            } else {
                // The same text, shifted up by the lines shown before.
                out.push_str(&format!(
                    "<div class=\"fb cont{}\" data-of=\"{}\" data-skip=\"{lines_before}\" data-align=\"{align}\" translate=\"no\" aria-hidden=\"true\" style=\"{geometry}\"><div class=\"in\" style=\"margin-top:-{}\">{inner}</div></div>\n",
                    if frag_idx + 1 == frags.len() { " tail" } else { "" },
                    block.id,
                    units.len(lines_before as f32 * lead),
                ));
            }
        }
        lines_before += n;
    }
    out
}

/// A reconstructed table laid out on its original grid.
fn table_box(table: &Table, units: &Units) -> String {
    let geom = &table.geom;
    let (box_width, box_height) = (geom.bbox.width().max(1.0), geom.bbox.height().max(1.0));
    let mut out = format!(
        "<div class=\"tb\" data-kind=\"table\" style=\"left:{};top:{};width:{};height:{};font-size:{}\"><table><colgroup>",
        units.x(geom.bbox.x0),
        units.y(geom.bbox.y0),
        units.x(box_width),
        units.y(box_height),
        units.len(geom.size),
    );
    for col in 0..table.n_cols {
        out.push_str(&format!(
            "<col style=\"width:{:.3}%\">",
            (geom.col_bounds[col + 1] - geom.col_bounds[col]) / box_width * 100.0
        ));
    }
    out.push_str("</colgroup>");
    let mut open_row: Option<usize> = None;
    table_walk(table, |row_idx, col, cell| {
        if open_row != Some(row_idx) {
            if open_row.is_some() {
                out.push_str("</tr>");
            }
            out.push_str(&format!(
                "<tr style=\"height:{:.3}%\">",
                (geom.row_bounds[row_idx + 1] - geom.row_bounds[row_idx]) / box_height * 100.0
            ));
            open_row = Some(row_idx);
        }
        let tag = if row_idx < table.header_rows { "th" } else { "td" };
        match cell {
            None => out.push_str(&format!(
                "<{tag} class=\"e{}\"></{tag}>",
                table_rule_classes(table, row_idx, col, 1, 1)
            )),
            Some(cell) => out.push_str(&table_cell(table, units, row_idx, cell, tag)),
        }
    });
    if open_row.is_some() {
        out.push_str("</tr>");
    }
    out.push_str("</table></div>\n");
    out
}

/// A non-empty cell of row `row_idx` of a table laid out on its grid, as a
/// `tag` (`th` or `td`) element. Its padding puts the text where it is in
/// the PDF.
fn table_cell(table: &Table, units: &Units, row_idx: usize, cell: &TableCell, tag: &str) -> String {
    let geom = &table.geom;
    let first_col = cell.col.min(table.n_cols - 1);
    let last_col = (cell.col + cell.colspan).min(table.n_cols) - 1;
    // Put the first line on the baseline it has in the PDF (cells
    // spanning rows are centred instead).
    let top = if cell.rowspan > 1 {
        0.0
    } else {
        (geom.row_baselines[row_idx] - BASELINE * geom.size - geom.row_bounds[row_idx]).max(0.0)
    };
    let (align, pad) = if cell.colspan > 1 {
        ("c", format!(" style=\"padding-top:{}\"", units.len(top)))
    } else {
        // Keep the text inside the band it occupies in the PDF.
        let left = (geom.col_text[first_col].0 - geom.col_bounds[first_col]).max(0.0);
        let right = (geom.col_bounds[last_col + 1] - geom.col_text[last_col].1).max(0.0);
        (
            &table.col_align[first_col][..1],
            format!(
                " style=\"padding:{} {} 0 {}\"",
                units.len(top),
                units.len(right),
                units.len(left)
            ),
        )
    };
    let attrs = span_attrs(cell);
    let (html, _) = spans_h95(&cell.spans, &[]);
    format!(
        "<{tag}{attrs} class=\"{align}{}{}\"{pad}>{html}</{tag}>",
        if cell.rowspan > 1 { " mid" } else { "" },
        table_rule_classes(table, row_idx, cell.col, cell.colspan, cell.rowspan),
    )
}

/// The glyph runs of a displayed formula.
fn equation(block: &Block, units: &Units) -> String {
    let mut out = format!(
        "<div class=\"eq\" data-kind=\"equation\" data-block=\"{}\" translate=\"no\">",
        block.id
    );
    for run in &block.eq_runs {
        let span = Span {
            text: run.text.clone(),
            style: run.style | MATH,
            href: None,
            color: run.color,
        };
        let html = span_html(&span, &run.text, &mut [].iter());
        let ink = run
            .ink
            .map(|ink| {
                format!(
                    " data-ink=\"{:.3},{:.3},{:.3},{:.3}\"",
                    ink.x0 / units.page_width * 100.0,
                    ink.y0 / units.page_height * 100.0,
                    ink.x1 / units.page_width * 100.0,
                    ink.y1 / units.page_height * 100.0
                )
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "<span class=\"g\" style=\"left:{};top:{};font-size:{}\" data-w=\"{}\"{ink}>{html}</span>",
            units.x(run.x),
            units.y(run.y - BASELINE * run.size),
            units.len(run.size),
            units.width_attr(run.width),
        ));
    }
    out.push_str("</div>\n");
    out
}

/// Adds the origins of the glyphs of `line` that are written as text to the
/// set of glyphs drawn as HTML (glyphs without Unicode value stay in the
/// picture).
fn add_origins(set: &mut HashSet<(i32, i32)>, line: &Line) {
    for glyph in &line.glyphs {
        // Glyphs without a Unicode value cannot be written as text.
        if glyph.text.contains('\u{FFFD}') {
            continue;
        }
        set.insert(origin_key(glyph.ox, glyph.oy));
        if let Some((x, y)) = glyph.extra {
            set.insert(origin_key(x, y));
        }
    }
}
