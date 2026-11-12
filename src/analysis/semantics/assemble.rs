//! Assembling the final [`Document`] from the classified nodes.

use super::links::*;
use super::patterns::*;
use super::text::*;
use super::*;
use crate::analysis::blocks::LIST_LABEL;
use crate::analysis::lines::split_at_gaps;
use crate::analysis::page::{DocStats, PageData};
use crate::analysis::pipeline::AnalyzeOptions;
use crate::analysis::tables::extract_grid;
use crate::backend::FontInfo;
use crate::common::geom::Rect;
use crate::model::FloatKind;
use crate::model::Line;
use crate::model::Loc;
use crate::model::*;
use std::collections::{HashMap, HashSet};

pub(crate) fn new_block(id: usize, kind: BlockKind, loc: Loc, bbox: Rect) -> Block {
    Block {
        id,
        kind,
        page: loc.page,
        bbox,
        spans: Vec::new(),
        rows: Vec::new(),
        anchor: None,
        loc,
        lines: Vec::new(),
        line_texts: Vec::new(),
        hlines: Vec::new(),
        table: None,
        inner_lines: Vec::new(),
        inner_spans: Vec::new(),
        eq_runs: Vec::new(),
    }
}

pub(crate) fn is_footnote_symbol(s: &str) -> bool {
    matches!(s, "∗" | "†" | "‡" | "§" | "¶" | "*")
}

pub(crate) fn build_document(
    nodes: Vec<Node>,
    pages: &[PageData],
    stats: &DocStats,
    fonts: &[FontInfo],
    mut meta: Meta,
    furniture: Vec<Vec<Line>>,
    opts: AnalyzeOptions,
) -> (Document, Vec<Vec<PLine>>) {
    let vocab = build_vocab(nodes.iter().filter_map(|n| match &n.kind {
        NodeKind::Text(t) | NodeKind::Footnote(t) => Some(t.lines.as_slice()),
        NodeKind::Float(f) => Some(f.caption.as_slice()),
        _ => None,
    }));
    let tb = TextBuilder { fonts, vocab: &vocab };

    let mut loose: Vec<Vec<PLine>> = vec![Vec::new(); pages.len()];
    for (pi, lines) in furniture.into_iter().enumerate() {
        for line in lines {
            let spans = tb.line_spans(&line);
            loose[pi].push(PLine {
                line,
                spans,
                kind: "header",
            });
        }
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut anchors: HashSet<String> = HashSet::new();
    let unique = |base: String, anchors: &mut HashSet<String>| -> String {
        let mut a = base.clone();
        let mut k = 2;
        while !anchors.insert(a.clone()) {
            a = format!("{base}-{k}");
            k += 1;
        }
        a
    };
    let mut title_parts: Vec<String> = Vec::new();
    let mut eq_counter = 0;

    for n in nodes {
        let id = blocks.len();
        let loc = n.loc;
        match n.kind {
            NodeKind::Text(t) => {
                let bbox = t.lines.iter().fold(Rect::EMPTY, |a, l| a.union(&l.bbox));
                let mut rows = Vec::new();
                let mut anchor = None;
                let (kind, built) = match n.class {
                    Class::Title => {
                        let b = tb.build(&t.lines, 0, BOLD);
                        title_parts.push(b.spans.iter().map(|x| x.text.as_str()).collect());
                        (BlockKind::Title, b)
                    }
                    Class::Authors => {
                        // One row per baseline; side-by-side pieces are
                        // separated by an em space.
                        let mut last_baseline = f32::NEG_INFINITY;
                        for l in &t.lines {
                            let mut spans = tb.line_spans(l);
                            if (l.baseline - last_baseline).abs() < 2.0 && !rows.is_empty() {
                                let row: &mut Vec<Span> = rows.last_mut().unwrap();
                                row.push(Span::plain("\u{2003}"));
                                row.append(&mut spans);
                            } else {
                                rows.push(spans);
                            }
                            last_baseline = l.baseline;
                        }
                        (BlockKind::Authors, tb.build(&t.lines, 0, 0))
                    }
                    Class::Note => (BlockKind::Note, tb.build(&t.lines, 0, 0)),
                    Class::Heading { level, number } => {
                        let b = tb.build(&t.lines, 0, BOLD);
                        let text: String = b.spans.iter().map(|x| x.text.as_str()).collect();
                        let base = match &number {
                            Some(num) => format!("sec-{num}"),
                            None => format!("sec-{}", slug(&text)),
                        };
                        anchor = Some(unique(base, &mut anchors));
                        (BlockKind::Heading { level }, b)
                    }
                    Class::ListItem { bullet } => {
                        let b = tb.build(&t.lines, if bullet { 1 } else { 0 }, 0);
                        let marker = if bullet {
                            None
                        } else {
                            let text: String = b.spans.iter().map(|x| x.text.as_str()).collect();
                            LIST_LABEL.find(&text).map(|m| m.as_str().trim().to_string())
                        };
                        (BlockKind::ListItem { marker }, b)
                    }
                    Class::Reference => {
                        let b = tb.build(&t.lines, 0, 0);
                        let text: String = b.spans.iter().map(|x| x.text.as_str()).collect();
                        let label = REF_LABEL
                            .captures(text.trim())
                            .map(|c| c.name("a").or(c.name("b")).unwrap().as_str().to_string());
                        if let Some(l) = &label {
                            anchor = Some(unique(format!("ref-{}", slug(l)), &mut anchors));
                        }
                        (BlockKind::Reference { label }, b)
                    }
                    Class::Paragraph { in_abstract } => {
                        let mut b = tb.build(&t.lines, 0, 0);
                        if n.strip_prefix > 0 {
                            strip_chars(&mut b.spans, n.strip_prefix);
                        }
                        (BlockKind::Paragraph { in_abstract }, b)
                    }
                    Class::Unknown => (BlockKind::Paragraph { in_abstract: false }, tb.build(&t.lines, 0, 0)),
                };
                let mut block = new_block(id, kind, loc, bbox);
                block.spans = built.spans;
                block.line_texts = built.line_texts;
                block.hlines = built.hlines;
                block.rows = rows;
                block.anchor = anchor;
                block.lines = t.lines;
                blocks.push(block);
            }
            NodeKind::Footnote(t) => {
                let bbox = t.lines.iter().fold(Rect::EMPTY, |a, l| a.union(&l.bbox));
                let first = &t.lines[0];
                let mut skip = first.glyphs.iter().take_while(|g| g.script == 1).count();
                if skip == 0 && first.glyphs.first().is_some_and(|g| is_footnote_symbol(&g.text)) {
                    skip = 1;
                }
                let marker: String = first.glyphs[..skip].iter().map(|g| g.text.as_str()).collect();
                let built = tb.build(&t.lines, skip, 0);
                let anchor = (!marker.is_empty()).then(|| unique(format!("fn-{}", slug_marker(&marker)), &mut anchors));
                let mut block = new_block(id, BlockKind::Footnote { marker }, loc, bbox);
                block.spans = built.spans;
                block.line_texts = built.line_texts;
                block.hlines = built.hlines;
                block.anchor = anchor;
                block.lines = t.lines;
                blocks.push(block);
            }
            NodeKind::Equation(e) => {
                let mut lines = e.lines;
                lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
                let alt = lines.iter().map(|l| l.text()).collect::<Vec<_>>().join(" ");
                let rect = e.bbox.inflate(2.5, 2.5).intersect(&pages[loc.page].rect());
                let anchor = match &e.number {
                    Some(num) => Some(unique(format!("eq-{}", num.trim_matches(['(', ')'])), &mut anchors)),
                    None => {
                        eq_counter += 1;
                        Some(unique(format!("eq-u{eq_counter}"), &mut anchors))
                    }
                };
                if let Some(line) = e.number_line {
                    let spans = tb.line_spans(&line);
                    loose[loc.page].push(PLine {
                        line,
                        spans,
                        kind: "eqno",
                    });
                }
                let mut block = new_block(
                    id,
                    BlockKind::Equation {
                        image: ImageSpec {
                            page: loc.page,
                            rect,
                            transparent: true,
                        },
                        number: e.number,
                    },
                    loc,
                    rect,
                );
                block.spans = vec![Span::plain(alt)];
                block.line_texts = lines.iter().map(|l| l.text()).collect();
                block.eq_runs = tb.eq_runs(&lines);
                block.anchor = anchor;
                block.lines = lines;
                blocks.push(block);
            }
            NodeKind::Float(f) => {
                let built = tb.build(&f.caption, 0, 0);
                let anchor = f.label.as_ref().map(|label| {
                    let num = label.rsplit(' ').next().unwrap_or("");
                    let prefix = match f.kind {
                        FloatKind::Table => "tab",
                        FloatKind::Algorithm => "alg",
                        _ => "fig",
                    };
                    unique(format!("{prefix}-{num}"), &mut anchors)
                });
                let mut rows = Vec::new();
                if !f.inner_text.is_empty() {
                    rows.push(vec![Span::plain(f.inner_text.clone())]);
                }
                let tabular = f.kind == FloatKind::Table || (f.kind == FloatKind::Graphic && f.table_like);
                let table = match f.region {
                    Some(region) if opts.tables && tabular && !f.complex => {
                        // Cells can be closer together than the gap at which
                        // lines are broken; split more finely. Pieces that
                        // end up in the same column are joined again.
                        let pieces: Vec<Line> = f
                            .inner_lines
                            .iter()
                            .flat_map(|l| split_at_gaps(l, 0.55 * l.size))
                            .collect();
                        extract_grid(region, &pieces, &f.hrules, &f.vrules).map(|g| tb.table(g))
                    }
                    _ => None,
                };
                let mut block = new_block(
                    id,
                    BlockKind::Figure {
                        kind: f.kind,
                        image: f.region.map(|rect| ImageSpec {
                            page: f.page,
                            rect,
                            transparent: false,
                        }),
                        label: f.label.clone(),
                    },
                    loc,
                    f.bbox,
                );
                block.spans = built.spans;
                block.line_texts = built.line_texts;
                block.hlines = built.hlines;
                block.rows = rows;
                block.anchor = anchor;
                block.lines = f.caption;
                block.inner_spans = f.inner_lines.iter().map(|l| tb.line_spans(l)).collect();
                block.inner_lines = f.inner_lines;
                block.table = table;
                blocks.push(block);
            }
        }
    }

    // Footnote marks in the running text.
    let fn_markers: HashMap<String, String> = blocks
        .iter()
        .filter_map(|b| match (&b.kind, &b.anchor) {
            (BlockKind::Footnote { marker }, Some(a)) if marker.chars().all(|c| c.is_ascii_digit()) => {
                Some((marker.clone(), a.clone()))
            }
            _ => None,
        })
        .collect();

    for b in blocks.iter_mut() {
        let own = b.anchor.clone();
        let is_footnote = matches!(b.kind, BlockKind::Footnote { .. });
        let linkable = !matches!(
            b.kind,
            BlockKind::Equation { .. } | BlockKind::Title | BlockKind::Heading { .. }
        );
        if linkable {
            let spans = std::mem::take(&mut b.spans);
            b.spans = link_spans(spans, &anchors, own.as_deref());
            let rows = std::mem::take(&mut b.rows);
            b.rows = rows.into_iter().map(|r| link_spans(r, &anchors, None)).collect();
            for h in b.hlines.iter_mut() {
                let spans = std::mem::take(&mut h.spans);
                h.spans = link_spans(spans, &anchors, own.as_deref());
            }
            if let Some(t) = b.table.as_mut() {
                for cell in t.rows.iter_mut().flatten() {
                    let spans = std::mem::take(&mut cell.spans);
                    cell.spans = link_spans(spans, &anchors, None);
                }
            }
        }
        if !is_footnote {
            for i in 0..b.spans.len() {
                let s = &b.spans[i];
                if s.style & SUP != 0 && s.style & MATH == 0 && s.href.is_none() {
                    // A footnote mark follows a word or punctuation; a digit
                    // or a single letter before it means an exponent (x², c²).
                    let exponent = i == 0 || b.spans[i - 1].style & SUP != 0 || {
                        let prev = b.spans[i - 1].text.trim_end();
                        let run = prev.chars().rev().take_while(|c| c.is_alphabetic()).count();
                        match prev.chars().last() {
                            Some(c) if c.is_ascii_digit() => true,
                            Some(c) if c.is_alphabetic() => run < 2,
                            Some(_) => false,
                            None => true,
                        }
                    };
                    if let (Some(a), false) = (fn_markers.get(s.text.trim()), exponent) {
                        b.spans[i].href = Some(format!("#{a}"));
                    }
                }
            }
        }
    }

    if !title_parts.is_empty() {
        meta.title = Some(title_parts.join(" ").split_whitespace().collect::<Vec<_>>().join(" "));
    }
    let doc = Document {
        meta,
        pages: pages
            .iter()
            .map(|p| PageInfo {
                width: p.width,
                height: p.height,
            })
            .collect(),
        body_size: stats.body_size,
        leading: stats.leading,
        body_font: fonts
            .get(stats.body_font as usize)
            .map(|f| f.name.clone())
            .unwrap_or_default(),
        blocks,
    };
    (doc, loose)
}

pub(crate) fn slug_marker(m: &str) -> String {
    m.chars()
        .map(|c| match c {
            '∗' | '*' => "star".to_string(),
            '†' => "dagger".to_string(),
            '‡' => "ddagger".to_string(),
            '§' => "sect".to_string(),
            '¶' => "para".to_string(),
            c if c.is_alphanumeric() => c.to_string(),
            _ => "x".to_string(),
        })
        .collect()
}
