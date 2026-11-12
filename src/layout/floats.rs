//! Detection of figures, tables and other graphics, and their captions.
//!
//! Vector drawings and images are clustered into components, captions are
//! found by their label ("Figure 3:"), and each caption claims the nearest
//! component. Everything inside a claimed region is rendered as an image
//! later, so the text inside it is removed from the text flow.

use super::lines::Line;
use super::page::{DocStats, PageData};
use crate::geom::Rect;
use crate::pdf::FontInfo;
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FloatKind {
    Figure,
    Table,
    Algorithm,
    /// A drawing or image without a caption.
    Graphic,
}

#[derive(Debug, Clone)]
pub struct Float {
    pub kind: FloatKind,
    pub page: usize,
    /// Area of the page to render as the float's image.
    pub region: Option<Rect>,
    pub caption: Vec<Line>,
    /// E.g. "Figure 2".
    pub label: Option<String>,
    /// Region plus caption.
    pub bbox: Rect,
    /// Text found inside the region (labels, table cells), for alt text.
    pub inner_text: String,
}

static CAPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<kind>Supplementary\s+Figure|Supplementary\s+Table|Extended\s+Data\s+Fig\.?|Extended\s+Data\s+Table|Figure|Fig\.?|FIGURE|FIG\.?|Table|TABLE|Tab\.|Algorithm|ALGORITHM|Listing|Scheme|Chart|Plate)\s*(?P<num>\d+[a-z]?|[IVXLC]+|[A-Z]\.?\d+|S\d+)\s*(?P<punct>[:.|—–-]|$)?",
    )
    .unwrap()
});

static HEADING_LIKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\d{1,2}(?:\.\d{1,2})*\.?|[IVX]+\.|[A-Z]\.)\s+\p{Lu}").unwrap());

fn caption_kind(word: &str) -> FloatKind {
    let w = word.to_ascii_lowercase();
    if w.contains("tab") {
        FloatKind::Table
    } else if w.contains("algorithm") || w.contains("listing") {
        FloatKind::Algorithm
    } else {
        FloatKind::Figure
    }
}

/// A line that looks like running prose (as opposed to a label inside a
/// figure or a table cell).
fn body_like(l: &Line, stats: &DocStats) -> bool {
    l.char_count() >= 45 && l.bbox.width() > 18.0 * stats.body_size
}

struct Cap {
    lines: Vec<usize>,
    bbox: Rect,
    kind: FloatKind,
    label: String,
}

fn find_captions(page: &PageData, fonts: &[FontInfo]) -> Vec<Cap> {
    let lines = &page.lines;
    let mut used = vec![false; lines.len()];
    let mut caps = Vec::new();
    // Lines are sorted by baseline.
    for i in 0..lines.len() {
        if used[i] {
            continue;
        }
        let l = &lines[i];
        let text = l.text();
        let Some(m) = CAPTION.captures(&text) else { continue };
        let label_len = m.get(0).unwrap().as_str().chars().filter(|c| !c.is_whitespace()).count();
        let has_punct = m.name("punct").is_some_and(|p| !p.as_str().is_empty());
        let label_alone = m.get(0).unwrap().end() == text.len();
        // Label set in bold while the caption text is not.
        let glyph_chars: Vec<(usize, bool)> = l
            .glyphs
            .iter()
            .map(|g| (g.text.chars().count(), fonts[g.font as usize].bold))
            .collect();
        let mut seen = 0;
        let (mut bold_label, mut bold_rest, mut n_label, mut n_rest) = (0, 0, 0, 0);
        for (n, bold) in glyph_chars {
            if seen < label_len {
                n_label += 1;
                bold_label += bold as usize;
            } else {
                n_rest += 1;
                bold_rest += bold as usize;
            }
            seen += n;
        }
        let label_bold = n_label > 0 && bold_label * 10 >= n_label * 8 && (n_rest == 0 || bold_rest * 2 < n_rest);

        if !(has_punct || label_alone || label_bold) {
            continue;
        }

        // Reject lines that continue a paragraph: a line of the same size
        // right above, at normal leading, that overlaps horizontally.
        let continues = lines[..i].iter().rev().take(40).any(|a| {
            let dy = l.baseline - a.baseline;
            dy > 0.4 * l.size
                && dy < 1.45 * l.size
                && (a.size - l.size).abs() < 0.3
                && a.bbox.x_overlap(&l.bbox) > 0.6 * l.bbox.width().min(a.bbox.width())
                && a.char_count() > 20
        });
        if continues && !label_bold {
            continue;
        }
        // "Figure 3. Shows ..." at the start of a line inside a sentence is
        // handled by the check above; a lone "Fig. 3" reference with trailing
        // lowercase text and no punctuation never gets here.

        let mut idxs = vec![i];
        let mut bbox = l.bbox;
        let mut prev = i;
        let mut leading: Option<f32> = None;
        loop {
            let p = &lines[prev];
            let mut next: Option<usize> = None;
            for (j, n) in lines.iter().enumerate().skip(prev + 1) {
                let dy = n.baseline - p.baseline;
                let max_dy = match leading {
                    Some(lead) => 1.15 * lead + 0.5,
                    None => 1.38 * l.size,
                };
                if dy > max_dy {
                    break;
                }
                if used[j] || dy < 0.5 * l.size {
                    continue;
                }
                if (n.size - l.size).abs() > 0.4 {
                    continue;
                }
                if n.bbox.x_overlap(&bbox) <= 0.0 {
                    continue;
                }
                if n.bbox.x0 < bbox.x0 - 2.0 * l.size || n.bbox.x1 > bbox.x1 + 2.0 {
                    continue;
                }
                next = Some(j);
                break;
            }
            let Some(j) = next else { break };
            let n = &lines[j];
            if CAPTION.is_match(&n.text()) && n.bbox.x0 <= bbox.x0 + 2.0 {
                // The next caption directly below.
                let t = n.text();
                let c = CAPTION.captures(&t).unwrap();
                if c.name("punct").is_some_and(|p| !p.as_str().is_empty()) {
                    break;
                }
            }
            // The previous line ended early although the next word would
            // have fit: the caption is over.
            if idxs.len() > 1 && p.bbox.x1 + 0.3 * l.size + n.first_word_width() < bbox.x1 - 2.0 {
                break;
            }
            if leading.is_none() {
                leading = Some(n.baseline - p.baseline);
            }
            idxs.push(j);
            bbox = bbox.union(&n.bbox);
            prev = j;
            if idxs.len() > 60 {
                break;
            }
        }
        for &j in &idxs {
            used[j] = true;
        }
        caps.push(Cap {
            lines: idxs,
            bbox,
            kind: caption_kind(&m["kind"]),
            label: format!("{} {}", m["kind"].trim(), &m["num"]),
        });
    }
    caps
}

#[derive(Debug, Clone)]
struct Comp {
    bbox: Rect,
    n_paths: usize,
    n_images: usize,
    table_like: bool,
    n_rules: usize,
    owner: Option<usize>,
}

const CELL: f32 = 4.0;
const DILATE: f32 = 3.0;

fn cluster_graphics(page: &PageData) -> Vec<Comp> {
    let gw = (page.width / CELL).ceil() as usize + 2;
    let gh = (page.height / CELL).ceil() as usize + 2;
    let page_area = page.width * page.height;

    struct Elem {
        bbox: Rect,
        image: bool,
        rule: bool,
    }
    let mut elems: Vec<Elem> = Vec::new();
    for im in &page.images {
        if im.bbox.area() > 0.92 * page_area {
            continue;
        }
        elems.push(Elem { bbox: im.bbox, image: true, rule: false });
    }
    // Rotated text (axis labels) belongs to the drawing it is part of.
    for r in &page.rotated {
        let compact: String = r.text.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.starts_with("arXiv:") || r.bbox.is_unset() {
            continue;
        }
        elems.push(Elem { bbox: r.bbox, image: false, rule: false });
    }
    for p in &page.paths {
        if p.bbox.width() > 0.95 * page.width && p.bbox.height() > 0.95 * page.height {
            continue;
        }
        if p.filled && !p.stroked && p.is_white() {
            continue;
        }
        if p.color[3] < 8 {
            continue;
        }
        elems.push(Elem { bbox: p.bbox, image: false, rule: p.is_hrule() || p.is_vrule() });
    }
    if elems.is_empty() {
        return Vec::new();
    }

    let mut grid = vec![false; gw * gh];
    let cell_range = |r: &Rect| -> (usize, usize, usize, usize) {
        let cx0 = (((r.x0 - DILATE) / CELL).floor().max(0.0) as usize).min(gw - 1);
        let cy0 = (((r.y0 - DILATE) / CELL).floor().max(0.0) as usize).min(gh - 1);
        let cx1 = (((r.x1 + DILATE) / CELL).floor().max(0.0) as usize).min(gw - 1);
        let cy1 = (((r.y1 + DILATE) / CELL).floor().max(0.0) as usize).min(gh - 1);
        (cx0, cy0, cx1, cy1)
    };
    for e in &elems {
        let (cx0, cy0, cx1, cy1) = cell_range(&e.bbox);
        for cy in cy0..=cy1 {
            for c in &mut grid[cy * gw + cx0..=cy * gw + cx1] {
                *c = true;
            }
        }
    }
    // Connected components (4-connectivity) by flood fill.
    let mut label = vec![u32::MAX; gw * gh];
    let mut n_labels = 0u32;
    let mut stack = Vec::new();
    for start in 0..gw * gh {
        if !grid[start] || label[start] != u32::MAX {
            continue;
        }
        label[start] = n_labels;
        stack.push(start);
        while let Some(c) = stack.pop() {
            let (x, y) = (c % gw, c / gw);
            let mut visit = |n: usize| {
                if grid[n] && label[n] == u32::MAX {
                    label[n] = n_labels;
                    stack.push(n);
                }
            };
            if x > 0 {
                visit(c - 1);
            }
            if x + 1 < gw {
                visit(c + 1);
            }
            if y > 0 {
                visit(c - gw);
            }
            if y + 1 < gh {
                visit(c + gw);
            }
        }
        n_labels += 1;
    }
    let mut comps: Vec<Comp> = (0..n_labels)
        .map(|_| Comp {
            bbox: Rect::EMPTY,
            n_paths: 0,
            n_images: 0,
            table_like: false,
            n_rules: 0,
            owner: None,
        })
        .collect();
    for e in &elems {
        let (cx0, cy0, _, _) = cell_range(&e.bbox);
        let c = &mut comps[label[cy0 * gw + cx0] as usize];
        c.bbox = c.bbox.union(&e.bbox);
        if e.image {
            c.n_images += 1;
        } else {
            c.n_paths += 1;
            c.n_rules += e.rule as usize;
        }
    }
    // A drawing made (almost) only of straight rules is a ruled table.
    for c in comps.iter_mut() {
        c.table_like = c.n_images == 0 && c.n_paths >= 3 && c.n_rules * 10 >= c.n_paths * 9;
    }
    comps
}

/// Groups of horizontal rules with the same extent: tables set with booktabs
/// (or algorithm floats) have no other graphics.
fn rule_tables(page: &PageData, caps: &[Cap]) -> Vec<Comp> {
    let mut rules: Vec<Rect> = page
        .paths
        .iter()
        .filter(|p| p.is_hrule() && p.bbox.width() >= 30.0 && !(p.filled && !p.stroked && p.is_white()))
        .map(|p| p.bbox)
        .collect();
    if rules.len() < 2 || rules.len() > 600 {
        return Vec::new();
    }
    rules.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let mut group_of: Vec<usize> = (0..rules.len()).collect();
    fn find(g: &mut Vec<usize>, i: usize) -> usize {
        let mut r = i;
        while g[r] != r {
            r = g[r];
        }
        let mut c = i;
        while g[c] != r {
            let n = g[c];
            g[c] = r;
            c = n;
        }
        r
    }
    for i in 0..rules.len() {
        for j in i + 1..rules.len() {
            if (rules[i].x0 - rules[j].x0).abs() <= 3.0 && (rules[i].x1 - rules[j].x1).abs() <= 3.0 {
                let (a, b) = (find(&mut group_of, i), find(&mut group_of, j));
                if a != b {
                    group_of[b] = a;
                }
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<Rect>> = Default::default();
    for i in 0..rules.len() {
        let g = find(&mut group_of, i);
        groups.entry(g).or_default().push(rules[i]);
    }
    let mut out = Vec::new();
    for (_, rs) in groups {
        if rs.len() < 2 {
            continue;
        }
        // Split the group where a caption starts between two rules, or where
        // the rules are implausibly far apart.
        let mut cur: Vec<Rect> = vec![rs[0]];
        let flush = |cur: &mut Vec<Rect>, out: &mut Vec<Comp>| {
            if cur.len() >= 2 {
                let bbox = cur.iter().fold(Rect::EMPTY, |a, r| a.union(r));
                if bbox.height() >= 12.0 {
                    out.push(Comp {
                        bbox,
                        n_paths: cur.len(),
                        n_images: 0,
                        table_like: true,
                        n_rules: cur.len(),
                        owner: None,
                    });
                }
            }
            cur.clear();
        };
        for w in rs.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            let cap_between = caps.iter().any(|c| {
                c.bbox.cy() > a.y1 && c.bbox.cy() < b.y0 && c.bbox.x_overlap(a) > 0.3 * c.bbox.width().min(a.width())
            });
            if cap_between || b.y0 - a.y1 > 420.0 {
                flush(&mut cur, &mut out);
            }
            cur.push(*b);
        }
        flush(&mut cur, &mut out);
    }
    out
}

/// Collects evidence on whether table captions are set above or below the
/// table in this document: returns (votes for above, votes for below) from
/// captions that have a table on one side only.
pub fn table_caption_votes(page: &PageData, fonts: &[FontInfo]) -> (usize, usize) {
    let caps = find_captions(page, fonts);
    if !caps.iter().any(|c| c.kind == FloatKind::Table) {
        return (0, 0);
    }
    let mut comps: Vec<Comp> = cluster_graphics(page)
        .into_iter()
        .filter(|c| c.bbox.width() >= 12.0 && c.bbox.height() >= 12.0)
        .collect();
    comps.extend(rule_tables(page, &caps));
    let (mut above, mut below) = (0, 0);
    for cap in caps.iter().filter(|c| c.kind == FloatKind::Table) {
        let near = |k: &Comp, table_below: bool| {
            let ov = cap.bbox.x_overlap(&k.bbox) >= 0.5 * cap.bbox.width().min(k.bbox.width());
            let d = if table_below { k.bbox.y0 - cap.bbox.y1 } else { cap.bbox.y0 - k.bbox.y1 };
            ov && (-4.0..=30.0).contains(&d)
        };
        let has_below = comps.iter().any(|k| near(k, true));
        let has_above = comps.iter().any(|k| near(k, false));
        match (has_above, has_below) {
            (false, true) => above += 1,
            (true, false) => below += 1,
            _ => {}
        }
    }
    (above, below)
}

/// `table_caption_above`: the document's convention for table captions, if
/// it could be established.
pub fn detect_floats(
    page: &mut PageData,
    stats: &DocStats,
    fonts: &[FontInfo],
    table_caption_above: Option<bool>,
) -> Vec<Float> {
    let caps = find_captions(page, fonts);
    let is_caption_line: Vec<bool> = {
        let mut v = vec![false; page.lines.len()];
        for c in &caps {
            for &i in &c.lines {
                v[i] = true;
            }
        }
        v
    };

    let page_area = page.width * page.height;
    let mut comps: Vec<Comp> = cluster_graphics(page)
        .into_iter()
        .filter(|c| c.bbox.width() >= 12.0 && c.bbox.height() >= 12.0)
        .filter(|c| !(c.bbox.area() > 0.8 * page_area && page.lines.len() > 30))
        .collect();
    let tables = rule_tables(page, &caps);
    for t in tables {
        if comps.iter().any(|c| c.bbox.inflate(2.0, 2.0).contains(&t.bbox)) {
            continue;
        }
        comps.retain(|c| !t.bbox.inflate(2.0, 2.0).contains(&c.bbox));
        comps.push(t);
    }

    let body_between = |lines: &[Line], a: f32, b: f32, x: &Rect| -> bool {
        lines.iter().enumerate().any(|(i, l)| {
            !is_caption_line[i]
                && l.bbox.cy() > a
                && l.bbox.cy() < b
                && l.bbox.x_overlap(x) > 0.5 * l.bbox.width()
                && body_like(l, stats)
        })
    };

    // Seed assignment: (score, caption, component, above?).
    let mut pairs: Vec<(f32, usize, usize, bool)> = Vec::new();
    for (ci, cap) in caps.iter().enumerate() {
        for (ki, k) in comps.iter().enumerate() {
            let ov = cap.bbox.x_overlap(&k.bbox);
            if ov < 0.5 * cap.bbox.width().min(k.bbox.width()) {
                continue;
            }
            let (dist, above) = if k.bbox.y1 <= cap.bbox.y0 + 4.0 {
                ((cap.bbox.y0 - k.bbox.y1).max(0.0), true)
            } else if k.bbox.y0 >= cap.bbox.y1 - 4.0 {
                ((k.bbox.y0 - cap.bbox.y1).max(0.0), false)
            } else {
                continue;
            };
            if dist > 48.0 {
                continue;
            }
            let (a, b) = if above { (k.bbox.y1, cap.bbox.y0) } else { (cap.bbox.y1, k.bbox.y0) };
            if body_between(&page.lines, a, b, &cap.bbox) {
                continue;
            }
            // `above`: the drawing is above its caption.
            let penalty = match (cap.kind, above) {
                (FloatKind::Figure, false) => 14.0,
                (FloatKind::Table, drawing_above) => match table_caption_above {
                    Some(caption_above) if caption_above == drawing_above => 30.0,
                    Some(_) => 0.0,
                    None if drawing_above => 3.0,
                    None => 0.0,
                },
                (FloatKind::Algorithm, true) => 3.0,
                _ => 0.0,
            };
            // A ruled table fits a table caption, a drawing fits a figure.
            let mismatch = match (cap.kind, k.table_like) {
                (FloatKind::Figure, true) => 10.0,
                (FloatKind::Table, false) => 6.0,
                _ => 0.0,
            };
            // Captions are centred on (or aligned with) their float.
            let misalign = (k.bbox.cx() - cap.bbox.cx()).abs() / cap.bbox.width().max(k.bbox.width()).max(1.0) * 40.0;
            pairs.push((dist + penalty + mismatch + misalign, ci, ki, above));
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cap_region: Vec<Option<(Rect, bool)>> = vec![None; caps.len()];
    for (_, ci, ki, above) in pairs {
        if cap_region[ci].is_some() || comps[ki].owner.is_some() {
            continue;
        }
        comps[ki].owner = Some(ci);
        cap_region[ci] = Some((comps[ki].bbox, above));
    }

    // Growth: adjacent unclaimed components join the region.
    for (ci, cap) in caps.iter().enumerate() {
        let Some((mut region, above)) = cap_region[ci] else { continue };
        loop {
            let mut grew = false;
            for ki in 0..comps.len() {
                if comps[ki].owner.is_some() {
                    continue;
                }
                let k = comps[ki].bbox;
                let same_side = if above { k.y1 <= cap.bbox.y0 + 4.0 } else { k.y0 >= cap.bbox.y1 - 4.0 };
                if !same_side {
                    continue;
                }
                let x_ov = k.x_overlap(&region);
                let y_ov = k.y_overlap(&region);
                let touching = k.intersects(&region.inflate(3.0, 3.0));
                let vertical = x_ov > 0.3 * k.width().min(region.width()) && -y_ov <= 32.0 && {
                    let (a, b) = if k.cy() < region.cy() { (k.y1, region.y0) } else { (region.y1, k.y0) };
                    !body_between(&page.lines, a, b, &k)
                };
                let within_caption = k.x0 >= cap.bbox.x0 - 12.0 && k.x1 <= cap.bbox.x1 + 12.0;
                let horizontal = y_ov > 0.3 * k.height().min(region.height())
                    && (within_caption || (-x_ov <= 28.0 && cap.lines.len() == 1));
                if touching || vertical || horizontal {
                    comps[ki].owner = Some(ci);
                    region = region.union(&k);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        cap_region[ci] = Some((region, above));
    }

    // Uncaptioned graphics that are large enough to matter.
    let mut graphic_regions: Vec<Rect> = Vec::new();
    for k in comps.iter().filter(|k| k.owner.is_none()) {
        let (w, h) = (k.bbox.width(), k.bbox.height());
        let keep = (k.n_images >= 1 && w >= 30.0 && h >= 30.0)
            || (!k.table_like && k.n_paths >= 8 && w >= 60.0 && h >= 40.0)
            || (k.table_like && k.n_rules >= 3);
        if keep {
            // Merge with an overlapping graphic region if there is one.
            if let Some(r) = graphic_regions.iter_mut().find(|r| r.inflate(8.0, 8.0).intersects(&k.bbox)) {
                *r = r.union(&k.bbox);
            } else {
                graphic_regions.push(k.bbox);
            }
        }
    }

    // Left margin of the running text on this page.
    let text_left = page
        .lines
        .iter()
        .filter(|l| l.char_count() >= 45)
        .map(|l| l.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    let text_left = if text_left.is_finite() { text_left } else { 0.0 };

    // Text absorption.
    let mut absorbed: Vec<Option<usize>> = vec![None; page.lines.len()]; // float index
    struct Pending {
        kind: FloatKind,
        region: Option<Rect>,
        cap: Option<usize>,
        above: bool,
        table_like: bool,
    }
    let mut pending: Vec<Pending> = Vec::new();
    for (ci, cap) in caps.iter().enumerate() {
        let table_like = comps.iter().any(|k| k.owner == Some(ci) && k.table_like);
        pending.push(Pending {
            kind: cap.kind,
            region: cap_region[ci].map(|r| r.0),
            cap: Some(ci),
            above: cap_region[ci].map(|r| r.1).unwrap_or(true),
            table_like,
        });
    }
    for r in graphic_regions {
        pending.push(Pending {
            kind: FloatKind::Graphic,
            region: Some(r),
            cap: None,
            above: true,
            table_like: false,
        });
    }

    // Fallback for tables/algorithms without any graphics: take the tabular
    // rows next to the caption.
    for p in pending.iter_mut() {
        if p.region.is_some() || !matches!(p.kind, FloatKind::Table) {
            continue;
        }
        let cap = &caps[p.cap.unwrap()];
        let prose = |l: &Line| l.char_count() >= 50 && l.bbox.width() >= 0.8 * cap.bbox.width().max(150.0);
        for &down in &[true, false] {
            let mut region = Rect::EMPTY;
            let mut edge = if down { cap.bbox.y1 } else { cap.bbox.y0 };
            let mut rows = 0;
            let mut cand: Vec<&Line> = page
                .lines
                .iter()
                .enumerate()
                .filter(|(i, l)| {
                    !is_caption_line[*i]
                        && l.bbox.x_overlap(&cap.bbox.inflate(40.0, 0.0)) > 0.0
                        && if down { l.bbox.y0 >= cap.bbox.y1 - 2.0 } else { l.bbox.y1 <= cap.bbox.y0 + 2.0 }
                })
                .map(|(_, l)| l)
                .collect();
            cand.sort_by(|a, b| if down { a.bbox.y0.total_cmp(&b.bbox.y0) } else { b.bbox.y1.total_cmp(&a.bbox.y1) });
            for l in cand {
                let gap = if down { l.bbox.y0 - edge } else { edge - l.bbox.y1 };
                if gap > 2.4 * stats.body_size || prose(l) {
                    break;
                }
                region = region.union(&l.bbox);
                edge = if down { edge.max(l.bbox.y1) } else { edge.min(l.bbox.y0) };
                rows += 1;
            }
            if rows >= 3 && region.height() > 2.5 * stats.body_size {
                p.region = Some(region);
                p.above = !down;
                p.table_like = true;
                break;
            }
        }
    }

    for (fi, p) in pending.iter_mut().enumerate() {
        let Some(mut region) = p.region else { continue };
        for _ in 0..4 {
            let mut grew = false;
            let probe = region.inflate(5.0, 5.0);
            for (i, l) in page.lines.iter().enumerate() {
                if is_caption_line[i] || absorbed[i].is_some() {
                    continue;
                }
                let inside = region.inflate(1.0, 1.0).contains(&l.bbox);
                let touches = (probe.y_overlap(&l.bbox) >= 0.4 * l.bbox.height()
                    && probe.x_overlap(&l.bbox) >= 0.5 * l.bbox.width())
                    || probe.contains_point(l.bbox.cx(), l.bbox.cy());
                if inside || (touches && (!body_like(l, stats) || p.table_like)) {
                    // Prose that merely touches the region is not part of it.
                    if !inside && body_like(l, stats) && !region.inflate(1.0, 1.0).contains_point(l.bbox.cx(), l.bbox.cy()) {
                        continue;
                    }
                    absorbed[i] = Some(fi);
                    let u = region.union(&l.ink.union(&l.bbox));
                    if u != region {
                        region = u;
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }
        // Short isolated labels right above or below the drawing (titles of
        // sub-figures), as long as they are not headings or paragraph ends.
        if !p.table_like {
            let region_before = region;
            let mut grown = region;
            for (i, l) in page.lines.iter().enumerate() {
                let region = region_before;
                if is_caption_line[i] || absorbed[i].is_some() || l.char_count() >= 45 {
                    continue;
                }
                let vgap = if l.bbox.cy() < region.cy() { region.y0 - l.bbox.y1 } else { l.bbox.y0 - region.y1 };
                let hgap = if l.bbox.cx() < region.cx() { region.x0 - l.bbox.x1 } else { l.bbox.x0 - region.x1 };
                let stacked = (-2.0..=14.0).contains(&vgap) && l.bbox.x_overlap(&region) >= 0.8 * l.bbox.width();
                let beside = (-2.0..=16.0).contains(&hgap)
                    && l.bbox.y_overlap(&region) >= 0.8 * l.bbox.height()
                    && l.char_count() < 30;
                if !(stacked || beside) {
                    continue;
                }
                // A bold line may be a section heading -- unless it is
                // centred over the drawing and away from the text margin,
                // which makes it the drawing's title.
                let centred_title = (l.bbox.cx() - region.cx()).abs() <= 0.1 * region.width().max(40.0)
                    && l.bbox.x0 > text_left + 15.0
                    && stacked;
                let heading_like = l.size >= stats.body_size - 0.5
                    && (HEADING_LIKE.is_match(&l.text())
                        || (l.char_count() >= 4 && l.font_frac(fonts, |f| f.bold) >= 0.6 && !centred_title));
                if heading_like {
                    continue;
                }
                // Part of a paragraph: another line of the same size at
                // normal leading on the far side.
                let attached = page.lines.iter().enumerate().any(|(j, o)| {
                    j != i
                        && absorbed[j].is_none()
                        && (o.size - l.size).abs() < 0.4
                        && (o.baseline - l.baseline).abs() > 0.5 * l.size
                        && (o.baseline - l.baseline).abs() < 1.6 * l.size
                        && o.bbox.x_overlap(&l.bbox) > 0.0
                        && o.char_count() >= 30
                });
                if !attached {
                    absorbed[i] = Some(fi);
                    grown = grown.union(&l.bbox);
                }
            }
            region = grown;
        }
        // Sub-captions and labels between the drawing and the caption.
        if let Some(ci) = p.cap {
            let cap = &caps[ci];
            let (a, b) = if p.above { (region.y1 - 2.0, cap.bbox.y0 + 2.0) } else { (cap.bbox.y1 - 2.0, region.y0 + 2.0) };
            for (i, l) in page.lines.iter().enumerate() {
                if is_caption_line[i] || absorbed[i].is_some() || body_like(l, stats) {
                    continue;
                }
                if l.bbox.y0 >= a && l.bbox.y1 <= b && l.bbox.x_overlap(&region) > 0.5 * l.bbox.width() {
                    absorbed[i] = Some(fi);
                    region = region.union(&l.bbox);
                }
            }
            // Never let the image overlap its own caption.
            if p.above {
                region.y1 = region.y1.min(cap.bbox.y0 - 0.5).max(region.y0 + 1.0);
            } else {
                region.y0 = region.y0.max(cap.bbox.y1 + 0.5).min(region.y1 - 1.0);
            }
        }
        // ... nor any other caption on the page.
        let mut region = region.inflate(3.0, 3.0).intersect(&page.rect());
        for (ci, cap) in caps.iter().enumerate() {
            if p.cap == Some(ci) || !region.intersects(&cap.bbox) {
                continue;
            }
            if cap.bbox.cy() < region.cy() {
                region.y0 = region.y0.max(cap.bbox.y1 + 0.5).min(region.y1 - 1.0);
            } else {
                region.y1 = region.y1.min(cap.bbox.y0 - 0.5).max(region.y0 + 1.0);
            }
        }
        p.region = Some(region);
    }

    let mut floats: Vec<Float> = Vec::new();
    for (fi, p) in pending.iter().enumerate() {
        let caption: Vec<Line> = p
            .cap
            .map(|ci| caps[ci].lines.iter().map(|&i| page.lines[i].clone()).collect())
            .unwrap_or_default();
        let mut inner: Vec<&Line> = page
            .lines
            .iter()
            .enumerate()
            .filter(|(i, _)| absorbed[*i] == Some(fi))
            .map(|(_, l)| l)
            .collect();
        inner.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
        let inner_text = inner.iter().map(|l| l.text()).collect::<Vec<_>>().join(" ");
        let mut bbox = p.region.unwrap_or(Rect::EMPTY);
        if let Some(ci) = p.cap {
            bbox = bbox.union(&caps[ci].bbox);
        }
        if bbox.is_unset() {
            continue;
        }
        floats.push(Float {
            kind: p.kind,
            page: page.index,
            region: p.region,
            caption,
            label: p.cap.map(|ci| caps[ci].label.clone()),
            bbox,
            inner_text,
        });
    }

    // Remove caption and absorbed lines from the text flow.
    let mut i = 0;
    page.lines.retain(|_| {
        let keep = !is_caption_line[i] && absorbed[i].is_none();
        i += 1;
        keep
    });
    floats
}
