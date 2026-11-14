//! Stacked glyphs inside a line of text: inline fractions (`β₁²/√β₂`),
//! exponents that are fractions (`t^-½`), a subscript under a superscript
//! (`D∞²`) or limits on an inline sum.
//!
//! Written as running text, such glyphs come out side by side and the text
//! no longer matches the fraction bars and radicals of the page picture. In
//! hard-95 mode they are drawn glyph by glyph at their place in the PDF,
//! inside the line; the rest of the line stays flowing text.
//!
//! Glyphs on the baseline whose size differs from the line's (small
//! capitals, a big operator in the text) keep their own size.

use super::{Units, gaps_of, span_html, spans_h95};
use crate::model::{Line, SUB, SUP, Span};

/// A glyph is raised or lowered when its baseline is further than this (in
/// em of the line font size) from the baseline of its line.
const OFF_BASELINE_EM: f32 = 0.1;
/// Two raised or lowered glyphs are stacked when they overlap horizontally
/// by more than this (in points)...
const STACK_MIN_OVERLAP_PT: f32 = 0.5;
/// ...and their baselines are further apart than this (in em of the line
/// font size): a numerator over a denominator, a superscript over a
/// subscript. Less than that is one glyph slightly lower than the other.
const STACK_MIN_RISE_EM: f32 = 0.4;
/// A glyph on the baseline keeps its own font size when it differs from the
/// font size of the line by more than this fraction (small capitals are
/// about 80% of the capitals).
const RESIZED_MIN_RATIO: f32 = 0.08;

/// Consecutive glyphs of a line drawn at their own place.
struct Cluster {
    /// Index of the first glyph.
    first: usize,
    /// Index after the last glyph.
    end: usize,
    /// Range of the characters of the line text (the concatenated spans)
    /// the glyphs stand for, spaces between them included.
    chars: std::ops::Range<usize>,
}

/// Text of one span waiting to be written, all at one size.
#[derive(Default)]
struct Plain {
    /// The characters.
    text: String,
    /// Their own font size (points), or `None` for the size of the line.
    size: Option<f32>,
}

impl Plain {
    /// Writes the pending text of `span` (if any) and clears it.
    fn flush(&mut self, out: &mut String, span: &Span, gaps: &mut std::slice::Iter<'_, f32>, units: &Units) {
        if self.text.is_empty() {
            return;
        }
        let html = span_html(span, &self.text, gaps);
        match self.size {
            Some(size) => out.push_str(&format!("<span style=\"font-size:{}\">{html}</span>", units.len(size))),
            None => out.push_str(&html),
        }
        self.text.clear();
    }
}

/// The styled text of a line as HTML, with its stacked glyphs (if any)
/// drawn at their place and its resized glyphs at their size. Returns the
/// markup and whether the text ended with a space (which the caller places
/// outside the line element).
///
/// Lines without stacked or resized glyphs, and lines whose text cannot be
/// matched to their glyphs, are written as plain styled text.
pub(super) fn line_html(line: &Line, spans: &[Span], units: &Units) -> (String, bool) {
    let gaps = gaps_of(line);
    let runs = stacked_runs(line);
    if runs.is_empty() && !(0..line.glyphs.len()).any(|idx| own_size(line, idx).is_some()) {
        return spans_h95(spans, &gaps);
    }
    let Some(glyph_of_char) = glyph_of_chars(line, spans) else {
        return spans_h95(spans, &gaps);
    };
    let clusters = clusters_of(&runs, &glyph_of_char);
    write_line(line, spans, &clusters, &glyph_of_char, &gaps, units)
}

/// Runs of consecutive raised or lowered glyphs that contain two stacked
/// glyphs, as (first, end) glyph indices. Runs with a glyph that has no
/// Unicode value are left out (such glyphs stay in the page picture).
fn stacked_runs(line: &Line) -> Vec<(usize, usize)> {
    let off_baseline = |idx: usize| (line.glyphs[idx].y - line.baseline).abs() > OFF_BASELINE_EM * line.size;
    let mut runs = Vec::new();
    let mut idx = 0;
    while idx < line.glyphs.len() {
        if !off_baseline(idx) {
            idx += 1;
            continue;
        }
        let first = idx;
        while idx < line.glyphs.len() && off_baseline(idx) {
            idx += 1;
        }
        let glyphs = &line.glyphs[first..idx];
        if has_stacked_pair(line, first, idx) && !glyphs.iter().any(|glyph| glyph.text.contains('\u{FFFD}')) {
            runs.push((first, idx));
        }
    }
    runs
}

/// The font size of glyph `idx` of `line` when the glyph sits on the
/// baseline but is clearly smaller or larger than the line (see
/// [`RESIZED_MIN_RATIO`]); `None` otherwise.
fn own_size(line: &Line, idx: usize) -> Option<f32> {
    let glyph = &line.glyphs[idx];
    let on_baseline = (glyph.y - line.baseline).abs() <= OFF_BASELINE_EM * line.size;
    (on_baseline && (glyph.size - line.size).abs() > RESIZED_MIN_RATIO * line.size).then_some(glyph.size)
}

/// Whether two glyphs of `line.glyphs[first..end]` sit one over the other.
fn has_stacked_pair(line: &Line, first: usize, end: usize) -> bool {
    let glyphs = &line.glyphs[first..end];
    glyphs.iter().enumerate().any(|(i, upper)| {
        glyphs[i + 1..].iter().any(|other| {
            let overlap = upper.x1.min(other.x1) - upper.x0.max(other.x0);
            overlap > STACK_MIN_OVERLAP_PT && (upper.y - other.y).abs() > STACK_MIN_RISE_EM * line.size
        })
    })
}

/// For each character of the line text (the concatenated `spans`), the
/// index of the glyph it stands for (`None` for the spaces between words).
///
/// The text of a line is its glyphs in order, with a space where a glyph
/// has `space_before`; at the end of the line a soft hyphen may have been
/// removed or a space added. Returns `None` when the text is anything else.
fn glyph_of_chars(line: &Line, spans: &[Span]) -> Option<Vec<Option<usize>>> {
    let mut expected: Vec<(char, Option<usize>)> = Vec::new();
    for (glyph_idx, glyph) in line.glyphs.iter().enumerate() {
        if glyph.space_before && glyph_idx > 0 {
            expected.push((' ', None));
        }
        expected.extend(glyph.text.chars().map(|ch| (ch, Some(glyph_idx))));
    }
    let text: Vec<char> = spans.iter().flat_map(|span| span.text.chars()).collect();
    let matches = |len: usize| {
        len <= expected.len() && text.len() >= len && expected[..len].iter().zip(&text).all(|(exp, ch)| exp.0 == *ch)
    };
    let mut map: Vec<Option<usize>> = expected.iter().map(|exp| exp.1).collect();
    if text.len() == expected.len() && matches(expected.len()) {
        Some(map)
    } else if text.len() + 1 == expected.len() && expected.last()?.0 == '-' && matches(text.len()) {
        map.pop();
        Some(map)
    } else if text.len() == expected.len() + 1 && text.last() == Some(&' ') && matches(expected.len()) {
        map.push(None);
        Some(map)
    } else {
        None
    }
}

/// The clusters of the stacked runs, with the characters they cover.
fn clusters_of(runs: &[(usize, usize)], glyph_of_char: &[Option<usize>]) -> Vec<Cluster> {
    let in_run = |glyph: Option<usize>, first: usize, end: usize| glyph.is_some_and(|idx| first <= idx && idx < end);
    runs.iter()
        .filter_map(|&(first, end)| {
            let start = glyph_of_char.iter().position(|&glyph| in_run(glyph, first, end))?;
            let last = glyph_of_char.iter().rposition(|&glyph| in_run(glyph, first, end))?;
            Some(Cluster {
                first,
                end,
                chars: start..last + 1,
            })
        })
        .collect()
}

/// Writes the spans of a line, replacing the characters of each cluster by
/// the cluster drawn glyph by glyph, and setting resized glyphs (see
/// [`own_size`]) at their size.
fn write_line(
    line: &Line,
    spans: &[Span],
    clusters: &[Cluster],
    glyph_of_char: &[Option<usize>],
    gaps: &[f32],
    units: &Units,
) -> (String, bool) {
    let mut gaps = gaps.iter();
    let mut out = String::new();
    let mut trailing = false;
    let mut pos = 0usize;
    let mut next_cluster = 0usize;
    for (span_idx, span) in spans.iter().enumerate() {
        let mut text = span.text.as_str();
        if span_idx + 1 == spans.len() && text.ends_with(' ') {
            text = text.trim_end_matches(' ');
            trailing = true;
        }
        let mut plain = Plain::default();
        for ch in text.chars() {
            match clusters.get(next_cluster) {
                Some(cluster) if cluster.chars.contains(&pos) => {
                    plain.flush(&mut out, span, &mut gaps, units);
                    if pos + 1 == cluster.chars.end {
                        out.push_str(&cluster_html(line, spans, cluster, glyph_of_char, units));
                        next_cluster += 1;
                    }
                }
                _ => {
                    // Spaces (no glyph) keep the size of the text before them.
                    if let Some(glyph_idx) = glyph_of_char[pos] {
                        let size = own_size(line, glyph_idx);
                        if size != plain.size {
                            plain.flush(&mut out, span, &mut gaps, units);
                            plain.size = size;
                        }
                    }
                    plain.text.push(ch);
                }
            }
            pos += 1;
        }
        plain.flush(&mut out, span, &mut gaps, units);
    }
    (out, trailing)
}

/// The glyphs of a cluster, each in a box of its advance width placed at its
/// position in the PDF relative to the start of the cluster and to the
/// baseline of the line. Each glyph keeps the style of its span, without the
/// raising or lowering of superscripts and subscripts (its position does
/// that). Kept out of translation.
fn cluster_html(
    line: &Line,
    spans: &[Span],
    cluster: &Cluster,
    glyph_of_char: &[Option<usize>],
    units: &Units,
) -> String {
    let span_of_char = span_of_chars(spans);
    let glyphs = &line.glyphs[cluster.first..cluster.end];
    let mut cursor = glyphs[0].x0;
    let right = glyphs.iter().map(|glyph| glyph.x1).fold(cursor, f32::max);
    let mut out = String::from("<span class=\"st\" translate=\"no\">");
    for (offset, glyph) in glyphs.iter().enumerate() {
        let glyph_idx = cluster.first + offset;
        let char_idx = glyph_of_char.iter().position(|&glyph| glyph == Some(glyph_idx));
        let source = char_idx.and_then(|idx| span_of_char.get(idx)).map(|&idx| &spans[idx]);
        let style = Span {
            text: String::new(),
            style: source.map_or(0, |span| span.style) & !(SUP | SUB),
            href: source.and_then(|span| span.href.clone()),
            color: glyph.color,
        };
        let mut css = format!(
            "width:{};top:{};font-size:{}",
            units.len(glyph.x1 - glyph.x0),
            units.len(glyph.y - line.baseline),
            units.len(glyph.size),
        );
        if glyph.x0 != cursor {
            css.push_str(&format!(";margin-left:{}", units.len(glyph.x0 - cursor)));
        }
        if offset + 1 == glyphs.len() && right > glyph.x1 {
            css.push_str(&format!(";margin-right:{}", units.len(right - glyph.x1)));
        }
        cursor = glyph.x1;
        out.push_str(&format!(
            "<span class=\"sg\" style=\"{css}\">{}</span>",
            span_html(&style, &glyph.text, &mut [].iter())
        ));
    }
    out.push_str("</span>");
    out
}

/// For each character of the concatenated `spans`, the index of its span.
fn span_of_chars(spans: &[Span]) -> Vec<usize> {
    spans
        .iter()
        .enumerate()
        .flat_map(|(idx, span)| std::iter::repeat_n(idx, span.text.chars().count()))
        .collect()
}
