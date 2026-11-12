//! Rows of a column -> blocks: paragraphs, display equations, footnotes.

use super::lines::{Line, merge_row};
use super::order::{ItemRef, Leaf};
use super::page::{DocStats, PageData};
use crate::geom::Rect;
use crate::pdf::FontInfo;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Loc {
    pub page: usize,
    pub zone: usize,
    pub zone_cols: usize,
    pub col: usize,
    /// Index of the leaf on its page (unique per column fragment).
    pub leaf: usize,
}

#[derive(Debug, Clone)]
pub struct TextBlock {
    pub lines: Vec<Line>,
    pub loc: Loc,
    /// Left / right edge of the surrounding text group.
    pub left: f32,
    pub right: f32,
    pub centered: bool,
    /// Baseline distance to the previous row of the leaf.
    pub gap_before: f32,
    pub bbox: Rect,
}

#[derive(Debug, Clone)]
pub struct EquationBlock {
    pub lines: Vec<Line>,
    /// Ink extent of the equation without its number.
    pub bbox: Rect,
    pub number: Option<String>,
    /// The line holding the equation number, if any.
    pub number_line: Option<Line>,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub enum RawBlock {
    Text(TextBlock),
    Equation(EquationBlock),
    Float { idx: usize, loc: Loc },
    Footnote(TextBlock),
}

static EQNO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\((?:[A-Z]\.?)?\d+(?:\.\d+)*[a-z]?\)$").unwrap());
/// A bullet at the start of a line.
pub static BULLET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[•◦▪■–∗⋆·‣▶►○●◆\-]\s").unwrap());
/// An enumeration label at the start of a line.
pub static NUM_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\(\d{1,2}\)|\([a-z]\)|\([ivx]{1,4}\)|\d{1,2}[.)]|[a-z]\))\s").unwrap()
});
pub static LIST_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[•◦▪■–∗⋆·‣▶►○●◆\-]\s|\(\d{1,2}\)\s|\([a-z]\)\s|\([ivx]{1,4}\)\s|\d{1,2}[.)]\s|[a-z]\)\s)").unwrap()
});

fn is_math_char(c: char) -> bool {
    matches!(c,
        '=' | '+' | '−' | '×' | '÷' | '·' | '∑' | '∏' | '∫' | '√' | '∞' | '≤' | '≥' | '≠' | '≈'
        | '∈' | '∉' | '⊂' | '⊆' | '∪' | '∩' | '∧' | '∨' | '¬' | '→' | '←' | '↔' | '⇒' | '⇔'
        | '∀' | '∃' | '∂' | '∇' | '∝' | '∼' | '≡' | '⟨' | '⟩' | '‖' | '|' | '<' | '>' | '±'
        | '∓' | '⊕' | '⊗' | '∘' | '′' | '⊤' | '⊥' | '∥' | '≜' | '≪' | '≫' | '↦' | '∖')
        || ('\u{0391}'..='\u{03C9}').contains(&c)
        || ('\u{1D400}'..='\u{1D7FF}').contains(&c)
}

struct RowStats {
    n: usize,
    math: usize,
    prose: usize,
}

fn row_stats(lines: &[&Line], fonts: &[FontInfo]) -> RowStats {
    let mut st = RowStats { n: 0, math: 0, prose: 0 };
    for l in lines {
        let mut run = 0usize;
        for g in &l.glyphs {
            let f = &fonts[g.font as usize];
            let nchars = g.text.chars().count();
            st.n += nchars;
            let mathy = f.math || g.text.chars().any(is_math_char);
            if mathy {
                st.math += nchars;
            }
            let alpha = !mathy && g.script == 0 && g.text.chars().all(|c| c.is_alphabetic());
            if alpha && !g.space_before {
                run += nchars;
            } else {
                if run >= 4 {
                    st.prose += run;
                }
                run = if alpha { nchars } else { 0 };
            }
        }
        if run >= 4 {
            st.prose += run;
        }
    }
    st
}

enum Row {
    Text(Line),
    Equation { lines: Vec<Line>, bbox: Rect, number: Option<String>, number_line: Option<Line> },
    Float(usize),
}

fn percentile(mut v: Vec<f32>, q: f32, default: f32) -> f32 {
    if v.is_empty() {
        return default;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let idx = ((v.len() - 1) as f32 * q).round() as usize;
    v[idx]
}

fn ends_with_hyphen(l: &Line) -> bool {
    l.glyphs.last().is_some_and(|g| g.text.ends_with('-'))
}

fn bold_dominant(l: &Line, fonts: &[FontInfo]) -> bool {
    l.font_frac(fonts, |f| f.bold) >= 0.95
}

pub fn build_blocks(
    page: &PageData,
    leaves: &[Leaf],
    stats: &DocStats,
    fonts: &[FontInfo],
) -> Vec<RawBlock> {
    let mut out = Vec::new();
    for (leaf_idx, leaf) in leaves.iter().enumerate() {
        let loc = Loc {
            page: page.index,
            zone: leaf.zone,
            zone_cols: leaf.zone_cols,
            col: leaf.col,
            leaf: leaf_idx,
        };

        // --- gather rows ---
        struct Pre {
            lines: Vec<Line>,
            float: Option<usize>,
            bbox: Rect,
        }
        let mut pre: Vec<Pre> = Vec::new();
        for row in &leaf.rows {
            let mut lines = Vec::new();
            let mut float = None;
            for r in row {
                match *r {
                    ItemRef::Line(i) => lines.push(page.lines[i].clone()),
                    ItemRef::Float(i) => float = Some(i),
                }
            }
            let bbox = lines.iter().fold(Rect::EMPTY, |a, l| a.union(&l.bbox));
            pre.push(Pre { lines, float, bbox });
        }
        // Tiny orphan rows (pieces of inline math) join the row they overlap.
        let mut i = 0;
        while i < pre.len() {
            let tiny = pre[i].float.is_none()
                && pre[i].lines.iter().map(|l| l.char_count()).sum::<usize>() <= 4
                && pre[i].bbox.width() < 3.5 * stats.body_size;
            if tiny {
                let b = pre[i].bbox;
                let ok = |o: &Pre| {
                    o.float.is_none()
                        && o.bbox.y_overlap(&b) > 0.0
                        && b.x0 >= o.bbox.x0 - stats.body_size
                        && b.x1 <= o.bbox.x1 + stats.body_size
                        && o.lines.iter().map(|l| l.char_count()).sum::<usize>() > 4
                };
                let prev_ok = i > 0 && ok(&pre[i - 1]);
                let next_ok = i + 1 < pre.len() && ok(&pre[i + 1]);
                let target = match (prev_ok, next_ok) {
                    (true, true) => {
                        let dp = (pre[i - 1].bbox.cy() - b.cy()).abs();
                        let dn = (pre[i + 1].bbox.cy() - b.cy()).abs();
                        Some(if dp <= dn { i - 1 } else { i + 1 })
                    }
                    (true, false) => Some(i - 1),
                    (false, true) => Some(i + 1),
                    _ => None,
                };
                if let Some(t) = target {
                    let row = pre.remove(i);
                    let t = if t > i { t - 1 } else { t };
                    pre[t].lines.extend(row.lines);
                    pre[t].bbox = pre[t].bbox.union(&row.bbox);
                    continue;
                }
            }
            i += 1;
        }

        let text_x0: Vec<f32> = pre.iter().filter(|p| p.float.is_none()).map(|p| p.bbox.x0).collect();
        let text_x1: Vec<f32> = pre.iter().filter(|p| p.float.is_none()).map(|p| p.bbox.x1).collect();
        let leaf_left = text_x0.iter().copied().fold(f32::INFINITY, f32::min);
        let leaf_right = if text_x1.len() >= 8 {
            percentile(text_x1.clone(), 0.92, leaf.bbox.x1)
        } else {
            text_x1.iter().copied().fold(f32::NEG_INFINITY, f32::max)
        };
        let leaf_width = (leaf_right - leaf_left).max(1.0);
        let leaf_cx = (leaf_left + leaf_right) * 0.5;

        // --- classify rows ---
        let mut rows: Vec<Row> = Vec::new();
        let mut weak: Vec<Option<(Vec<Line>, Rect)>> = Vec::new();
        let ink_boxes: Vec<Rect> = pre
            .iter()
            .map(|p| p.lines.iter().fold(Rect::EMPTY, |a, l| a.union(&l.ink)))
            .collect();
        for (pi, p) in pre.into_iter().enumerate() {
            // White space above and below, as around a displayed formula.
            let isolated = {
                let me = ink_boxes[pi];
                let above = pi == 0 || ink_boxes[pi - 1].is_unset() || me.y0 - ink_boxes[pi - 1].y1 > 0.55 * stats.body_size;
                let below = pi + 1 >= ink_boxes.len()
                    || ink_boxes[pi + 1].is_unset()
                    || ink_boxes[pi + 1].y0 - me.y1 > 0.55 * stats.body_size;
                above && below
            };
            if let Some(f) = p.float {
                rows.push(Row::Float(f));
                weak.push(None);
                if p.lines.is_empty() {
                    continue;
                }
            }
            if p.lines.is_empty() {
                continue;
            }
            let mut lines = p.lines;
            lines.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
            // Equation number at the right margin.
            let mut number = None;
            if lines.len() >= 2 {
                let last = lines.last().unwrap();
                let t: String = last.text().chars().filter(|c| !c.is_whitespace()).collect();
                if EQNO.is_match(&t) && last.bbox.x1 >= leaf_right - 2.5 * stats.body_size {
                    number = Some(t);
                }
            }
            let content: Vec<&Line> = if number.is_some() {
                lines[..lines.len() - 1].iter().collect()
            } else {
                lines.iter().collect()
            };
            let st = row_stats(&content, fonts);
            let cb = content.iter().fold(Rect::EMPTY, |a, l| a.union(&l.bbox));
            let em = content.iter().map(|l| l.size).fold(0.0, f32::max).max(1.0);
            let left_indent = cb.x0 - leaf_left;
            let right_slack = leaf_right - cb.x1;
            let math_frac = st.math as f32 / st.n.max(1) as f32;
            let prose_frac = st.prose as f32 / st.n.max(1) as f32;
            let centered = (cb.cx() - leaf_cx).abs() <= (0.03 * leaf_width).max(4.0)
                && left_indent >= 1.2 * em
                && right_slack >= 1.2 * em;
            let multi = {
                let mut ys: Vec<f32> = content.iter().map(|l| l.baseline).collect();
                ys.sort_by(|a, b| a.total_cmp(b));
                ys.last().unwrap() - ys[0] > 0.25 * em
            };
            let is_eq = if number.is_some() {
                prose_frac < 0.6
            } else if math_frac >= 0.25 && prose_frac <= 0.5 && (centered || left_indent >= 3.0 * em) {
                true
            } else if math_frac >= 0.45 && prose_frac <= 0.15 && st.n >= 6 && (left_indent >= 1.0 * em || isolated) {
                // Almost nothing but symbols.
                true
            } else if centered
                && isolated
                && math_frac >= 0.08
                && prose_frac <= 0.8
                && content.iter().any(|l| l.glyphs.iter().any(|g| matches!(g.text.as_str(), "=" | "≤" | "≥" | "≈" | "∈" | "→" | "∝" | "≡" | "<" | ">")))
            {
                // A centred formula made mostly of operator names.
                true
            } else {
                multi && math_frac >= 0.3 && prose_frac <= 0.3 && left_indent >= 1.0 * em && right_slack >= 1.0 * em
            };
            // A formula-like line that did not qualify on its own; it is
            // promoted below if it sits right next to an equation row.
            let has_relation = content
                .iter()
                .any(|l| l.glyphs.iter().any(|g| matches!(g.text.as_str(), "=" | "≤" | "≥" | "≈" | "∈" | "→" | "∝" | "≡" | "<" | ">")));
            let weak_eq = !is_eq
                && ((has_relation
                    && left_indent >= 1.2 * em
                    && right_slack >= 1.2 * em
                    && st.n <= 110
                    && prose_frac <= 0.85)
                    || (math_frac >= 0.2 && prose_frac <= 0.3));
            let ink = content.iter().fold(Rect::EMPTY, |a, l| a.union(&l.ink));
            if is_eq {
                let n = content.len();
                let mut lines = lines;
                let number_line = if number.is_some() { lines.pop() } else { None };
                debug_assert_eq!(lines.len(), n);
                rows.push(Row::Equation { lines, bbox: ink, number, number_line });
                weak.push(None);
            } else {
                weak.push(weak_eq.then_some((lines.clone(), ink)));
                rows.push(Row::Text(merge_row(page.index, lines)));
            }
        }
        // Promote weak candidates adjacent to an equation (lines of a
        // multi-line display where only some lines look like math).
        loop {
            let mut changed = false;
            for i in 0..rows.len() {
                let Some((_, ink)) = &weak[i] else { continue };
                let near = |j: usize| match &rows[j] {
                    Row::Equation { bbox, .. } => {
                        let gap = if bbox.cy() < ink.cy() { ink.y0 - bbox.y1 } else { bbox.y0 - ink.y1 };
                        gap < 0.9 * stats.leading
                    }
                    _ => false,
                };
                if (i > 0 && near(i - 1)) || (i + 1 < rows.len() && near(i + 1)) {
                    let (lines, bbox) = weak[i].take().unwrap();
                    rows[i] = Row::Equation { lines, bbox, number: None, number_line: None };
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Merge vertically adjacent equation rows (multi-line equations,
        // numerators and denominators).
        let mut merged: Vec<Row> = Vec::new();
        for r in rows {
            if let Row::Equation { lines, bbox, number, number_line } = r {
                if let Some(Row::Equation { lines: pl, bbox: pb, number: pn, number_line: pnl }) = merged.last_mut() {
                    let gap = bbox.y0 - pb.y1;
                    let overlap = gap < 0.0;
                    if overlap || (gap < 0.9 * stats.leading && !(pn.is_some() && number.is_some())) {
                        pl.extend(lines);
                        *pb = pb.union(&bbox);
                        if pn.is_none() {
                            *pn = number;
                            *pnl = number_line;
                        } else if let Some(nl) = number_line {
                            // Keep the glyphs of a second number drawable.
                            pl.push(nl);
                        }
                        continue;
                    }
                }
                merged.push(Row::Equation { lines, bbox, number, number_line });
            } else {
                merged.push(r);
            }
        }
        let rows = merged;

        // --- footnotes ---
        // Index of the first row that belongs to the footnote area.
        let mut fn_start = rows.len();
        {
            let text_rows: Vec<(usize, &Line)> = rows
                .iter()
                .enumerate()
                .filter_map(|(i, r)| if let Row::Text(l) = r { Some((i, l)) } else { None })
                .collect();
            let small = |l: &Line| l.size <= stats.body_size - 0.4;
            // A footnote rule: short, left-aligned, with only small text below.
            let mut best: Option<usize> = None;
            for p in &page.paths {
                if !p.is_hrule() {
                    continue;
                }
                let w = p.bbox.width();
                if w < 20.0 || w > 0.62 * leaf_width.max(120.0) {
                    continue;
                }
                if (p.bbox.x0 - leaf_left).abs() > 8.0 {
                    continue;
                }
                if p.bbox.y0 < leaf.bbox.y0 || p.bbox.y0 > leaf.bbox.y1 {
                    continue;
                }
                let below: Vec<&(usize, &Line)> =
                    text_rows.iter().filter(|(_, l)| l.bbox.y0 > p.bbox.y0 - 2.0).collect();
                if below.is_empty() || !below.iter().all(|(_, l)| small(l)) {
                    continue;
                }
                // Nothing but small text rows may follow.
                let first = below[0].0;
                if rows[first..].iter().any(|r| !matches!(r, Row::Text(_))) {
                    continue;
                }
                if best.is_none_or(|b| first < b) {
                    best = Some(first);
                }
            }
            if let Some(b) = best {
                fn_start = b;
            } else {
                // No rule: a trailing run of small lines introduced by a
                // footnote mark, set apart from the text above.
                let mut k = rows.len();
                while k > 0 {
                    match &rows[k - 1] {
                        Row::Text(l) if l.size <= stats.body_size - 0.9 => k -= 1,
                        _ => break,
                    }
                }
                while k < rows.len() {
                    let Row::Text(l) = &rows[k] else { break };
                    let marked = l.starts_with_superscript()
                        || l.glyphs.first().is_some_and(|g| matches!(g.text.as_str(), "∗" | "†" | "‡" | "§" | "¶" | "*"));
                    let gap_ok = k == 0
                        || match &rows[k - 1] {
                            Row::Text(p) => l.baseline - p.baseline > 1.25 * stats.leading * l.size / stats.body_size,
                            _ => true,
                        };
                    if marked && gap_ok && k > 0 {
                        fn_start = k;
                        break;
                    }
                    k += 1;
                }
            }
        }

        // --- paragraphs ---
        let mut prev_baseline: Option<f32> = None;
        let mut group: Vec<(Line, f32)> = Vec::new(); // line, gap before
        let flush_group = |group: &mut Vec<(Line, f32)>, out: &mut Vec<RawBlock>, footnote: bool| {
            if group.is_empty() {
                return;
            }
            let lines: Vec<(Line, f32)> = std::mem::take(group);
            let left = lines.iter().map(|l| l.0.bbox.x0).fold(f32::INFINITY, f32::min);
            let right = lines.iter().map(|l| l.0.bbox.x1).fold(f32::NEG_INFINITY, f32::max);
            let cxs: Vec<f32> = lines.iter().map(|l| l.0.bbox.cx()).collect();
            let mcx = crate::geom::median(&cxs, 0.0);
            let centered = lines.len() >= 2
                && cxs.iter().all(|c| (c - mcx).abs() <= 2.5)
                && lines.iter().any(|l| (l.0.bbox.x0 - left).abs() > 3.0)
                && !footnote;

            let mut paras: Vec<Vec<(Line, f32)>> = Vec::new();
            for (line, gap) in lines {
                let Some(p) = paras.last_mut() else {
                    paras.push(vec![(line, gap)]);
                    continue;
                };
                let a = &p.last().unwrap().0;
                let em = line.size;
                let short_a = !ends_with_hyphen(a)
                    && a.bbox.x1 + 0.25 * em + line.first_word_width() < right - 1.0;
                let text = line.text();
                let bullet_b = BULLET.is_match(&text);
                let num_b = NUM_LABEL.is_match(&text);
                let marker_b = footnote
                    && (line.starts_with_superscript()
                        || line.glyphs.first().is_some_and(|g| matches!(g.text.as_str(), "∗" | "†" | "‡" | "§" | "¶" | "*")));
                let first_num = NUM_LABEL.is_match(&p[0].0.text());
                let aligned_first = (line.bbox.x0 - p[0].0.bbox.x0).abs() <= 0.6 * em;
                let brk = if centered {
                    false
                } else if marker_b || bullet_b {
                    true
                } else if p.len() == 1 {
                    short_a || (num_b && first_num && aligned_first)
                } else {
                    let rest_x0 = p[1..].iter().map(|l| l.0.bbox.x0).fold(f32::INFINITY, f32::min);
                    line.bbox.x0 > rest_x0 + 0.6 * em
                        || line.bbox.x0 < rest_x0 - 0.6 * em
                        || short_a
                        || (num_b && first_num && aligned_first)
                };
                if brk {
                    paras.push(vec![(line, gap)]);
                } else {
                    p.push((line, gap));
                }
            }
            for p in paras {
                let gap_before = p[0].1;
                let lines: Vec<Line> = p.into_iter().map(|l| l.0).collect();
                let bbox = lines.iter().fold(Rect::EMPTY, |a, l| a.union(&l.bbox));
                let tb = TextBlock {
                    lines,
                    loc,
                    left,
                    right,
                    centered,
                    gap_before,
                    bbox,
                };
                out.push(if footnote { RawBlock::Footnote(tb) } else { RawBlock::Text(tb) });
            }
        };

        let n_rows = rows.len();
        for (ri, row) in rows.into_iter().enumerate() {
            let footnote = ri >= fn_start;
            if ri == fn_start {
                flush_group(&mut group, &mut out, false);
                prev_baseline = None;
            }
            match row {
                Row::Float(idx) => {
                    flush_group(&mut group, &mut out, footnote);
                    out.push(RawBlock::Float { idx, loc });
                    prev_baseline = None;
                }
                Row::Equation { lines, bbox, number, number_line } => {
                    flush_group(&mut group, &mut out, footnote);
                    out.push(RawBlock::Equation(EquationBlock { lines, bbox, number, number_line, loc }));
                    prev_baseline = None;
                }
                Row::Text(line) => {
                    let gap = prev_baseline.map(|b| line.baseline - b).unwrap_or(f32::INFINITY);
                    if let Some((a, _)) = group.last() {
                        let expected = stats.leading * a.size.max(line.size) / stats.body_size;
                        let ink_gap = line.ink.y0 - a.ink.y1;
                        let big_gap = gap > 1.25 * expected + 1.5 && ink_gap > 0.45 * line.size;
                        let size_change = (a.size - line.size).abs() > 0.6;
                        let bold_flip = bold_dominant(a, fonts) != bold_dominant(&line, fonts)
                            && a.char_count() > 2
                            && line.char_count() > 2
                            // A bold run-in heading that fills a whole line is rare; a flip
                            // only separates blocks when the bold line stands alone.
                            && (bold_dominant(a, fonts) || gap > 1.1 * expected);
                        if big_gap || size_change || bold_flip {
                            flush_group(&mut group, &mut out, footnote);
                        }
                    }
                    prev_baseline = Some(line.baseline);
                    group.push((line, gap));
                }
            }
            if ri + 1 == n_rows {
                flush_group(&mut group, &mut out, footnote);
            }
        }
        flush_group(&mut group, &mut out, false);
    }
    out
}
