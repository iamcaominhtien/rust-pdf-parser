//! Reading order.
//!
//! A page is cut into horizontal zones; a zone is either single-column or is
//! split at its gutters into columns, which are ordered recursively. The
//! result is a list of leaves (column fragments) in reading order, each made
//! of rows of items.

use super::floats::Float;
use super::lines::Line;
use super::page::DocStats;
use crate::geom::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemRef {
    Line(usize),
    Float(usize),
}

#[derive(Debug, Clone, Copy)]
struct It {
    r: ItemRef,
    bbox: Rect,
    /// Vertical extent used for banding (tighter than the box).
    y0: f32,
    y1: f32,
}

#[derive(Debug, Clone)]
pub struct Leaf {
    /// Index of the top-level zone on the page.
    pub zone: usize,
    /// Number of columns of that zone.
    pub zone_cols: usize,
    /// Column of the zone this leaf belongs to.
    pub col: usize,
    pub rows: Vec<Vec<ItemRef>>,
    pub bbox: Rect,
}

fn x_intervals(items: &[It]) -> Vec<(f32, f32)> {
    let mut iv: Vec<(f32, f32)> = items.iter().map(|i| (i.bbox.x0, i.bbox.x1)).collect();
    iv.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f32, f32)> = Vec::new();
    for (a, b) in iv {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn gutters(items: &[It], min_gutter: f32) -> Vec<(f32, f32)> {
    let iv = x_intervals(items);
    iv.windows(2)
        .filter(|w| w[1].0 - w[0].1 >= min_gutter)
        .map(|w| (w[0].1, w[1].0))
        .collect()
}

fn weight(items: &[It]) -> usize {
    items
        .iter()
        .map(|i| match i.r {
            ItemRef::Line(_) => 1,
            ItemRef::Float(_) => 3,
        })
        .sum()
}

fn extent(items: &[It]) -> Rect {
    items.iter().fold(Rect::EMPTY, |a, i| a.union(&i.bbox))
}

/// Splits a zone at its widest valid gutter (recursively), returning the
/// columns from left to right. A single column means "do not split".
fn columns(items: Vec<It>, min_gutter: f32) -> Vec<Vec<It>> {
    if items.len() < 4 {
        return vec![items];
    }
    let ext = extent(&items);
    let mut gs = gutters(&items, min_gutter);
    gs.sort_by(|a, b| (b.1 - b.0).total_cmp(&(a.1 - a.0)));
    for (g0, g1) in gs {
        let mid = (g0 + g1) * 0.5;
        let (left, right): (Vec<It>, Vec<It>) = items.iter().partition(|i| i.bbox.cx() < mid);
        let (le, re) = (extent(&left), extent(&right));
        let wide_enough = le.width() >= 0.15 * ext.width() && re.width() >= 0.15 * ext.width();
        if weight(&left) >= 2 && weight(&right) >= 2 && wide_enough {
            let mut out = columns(left, min_gutter);
            out.extend(columns(right, min_gutter));
            return out;
        }
    }
    vec![items]
}

fn bands(mut items: Vec<It>) -> Vec<Vec<It>> {
    items.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let mut out: Vec<Vec<It>> = Vec::new();
    let mut cur_y1 = f32::NEG_INFINITY;
    for it in items {
        if out.is_empty() || it.y0 >= cur_y1 {
            out.push(vec![it]);
            cur_y1 = it.y1;
        } else {
            cur_y1 = cur_y1.max(it.y1);
            out.last_mut().unwrap().push(it);
        }
    }
    out
}

fn zones(items: Vec<It>, min_gutter: f32) -> Vec<Vec<It>> {
    let mut zones: Vec<Vec<It>> = Vec::new();
    let mut cur_has_gutter = false;
    for band in bands(items) {
        let band_gutter = !gutters(&band, min_gutter).is_empty();
        match zones.last_mut() {
            None => {
                cur_has_gutter = band_gutter;
                zones.push(band);
            }
            Some(z) => {
                if cur_has_gutter {
                    let mut u = z.clone();
                    u.extend(band.iter().copied());
                    if !gutters(&u, min_gutter).is_empty() {
                        *z = u;
                    } else {
                        cur_has_gutter = band_gutter;
                        zones.push(band);
                    }
                } else if band_gutter {
                    cur_has_gutter = true;
                    zones.push(band);
                } else {
                    z.extend(band);
                }
            }
        }
    }
    zones
}

/// Groups the items of a leaf into rows (top to bottom, left to right).
fn rows(mut items: Vec<It>) -> Vec<Vec<ItemRef>> {
    items.sort_by(|a, b| a.bbox.cy().total_cmp(&b.bbox.cy()));
    let mut out: Vec<(Rect, Vec<It>)> = Vec::new();
    for it in items {
        let joined = match out.last_mut() {
            Some((rb, row)) => {
                let ov = rb.y_overlap(&it.bbox);
                let is_float = matches!(it.r, ItemRef::Float(_)) || row.iter().any(|i| matches!(i.r, ItemRef::Float(_)));
                if !is_float && ov >= 0.5 * rb.height().min(it.bbox.height()) {
                    *rb = rb.union(&it.bbox);
                    row.push(it);
                    true
                } else {
                    false
                }
            }
            None => false,
        };
        if !joined {
            out.push((it.bbox, vec![it]));
        }
    }
    out.sort_by(|a, b| a.0.y0.total_cmp(&b.0.y0));
    out.into_iter()
        .map(|(_, mut row)| {
            row.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
            row.into_iter().map(|i| i.r).collect()
        })
        .collect()
}

enum Part {
    Leaf(Vec<It>),
    Split(Vec<Vec<It>>),
}

fn partition(items: Vec<It>, min_gutter: f32) -> Vec<Part> {
    let mut zs = zones(items, min_gutter);

    // A single-column zone directly above a multi-column zone belongs to it
    // when none of its items crosses the gutter (e.g. a heading on top of the
    // left column).
    let mut i = zs.len();
    while i >= 2 {
        i -= 1;
        let cols = columns(zs[i].clone(), min_gutter);
        if cols.len() < 2 {
            continue;
        }
        let prev_cols = columns(zs[i - 1].clone(), min_gutter);
        if prev_cols.len() >= 2 {
            continue;
        }
        let bounds: Vec<Rect> = cols.iter().map(|c| extent(c)).collect();
        let fits = zs[i - 1].iter().all(|it| {
            bounds
                .iter()
                .any(|b| it.bbox.x0 >= b.x0 - 3.0 && it.bbox.x1 <= b.x1 + 3.0)
        });
        if fits {
            let prev = zs.remove(i - 1);
            zs[i - 1].splice(0..0, prev);
        }
    }

    let mut parts: Vec<Part> = Vec::new();
    for z in zs {
        let cols = columns(z, min_gutter);
        if cols.len() == 1 {
            let items = cols.into_iter().next().unwrap();
            if let Some(Part::Leaf(prev)) = parts.last_mut() {
                prev.extend(items);
            } else {
                parts.push(Part::Leaf(items));
            }
        } else {
            parts.push(Part::Split(cols));
        }
    }
    parts
}

fn flatten(items: Vec<It>, min_gutter: f32, depth: u32, out: &mut Vec<Vec<It>>) {
    if depth > 8 {
        out.push(items);
        return;
    }
    for part in partition(items, min_gutter) {
        match part {
            Part::Leaf(items) => out.push(items),
            Part::Split(cols) => {
                for c in cols {
                    flatten(c, min_gutter, depth + 1, out);
                }
            }
        }
    }
}

pub fn order_page(lines: &[Line], floats: &[Float], stats: &DocStats) -> Vec<Leaf> {
    let mut items: Vec<It> = Vec::with_capacity(lines.len() + floats.len());
    for (i, l) in lines.iter().enumerate() {
        items.push(It {
            r: ItemRef::Line(i),
            bbox: l.bbox,
            y0: l.baseline - 0.6 * l.size,
            y1: l.baseline + 0.1 * l.size,
        });
    }
    for (i, f) in floats.iter().enumerate() {
        items.push(It {
            r: ItemRef::Float(i),
            bbox: f.bbox,
            y0: f.bbox.y0 + 1.0,
            y1: (f.bbox.y1 - 1.0).max(f.bbox.y0 + 1.5),
        });
    }
    let min_gutter = 0.7 * stats.body_size;

    let mut leaves = Vec::new();
    for (zone, part) in partition(items, min_gutter).into_iter().enumerate() {
        match part {
            Part::Leaf(items) => {
                let bbox = extent(&items);
                leaves.push(Leaf {
                    zone,
                    zone_cols: 1,
                    col: 0,
                    rows: rows(items),
                    bbox,
                });
            }
            Part::Split(cols) => {
                let n = cols.len();
                for (col, c) in cols.into_iter().enumerate() {
                    let mut sub = Vec::new();
                    flatten(c, min_gutter, 1, &mut sub);
                    for items in sub {
                        let bbox = extent(&items);
                        leaves.push(Leaf {
                            zone,
                            zone_cols: n,
                            col,
                            rows: rows(items),
                            bbox,
                        });
                    }
                }
            }
        }
    }
    leaves
}
