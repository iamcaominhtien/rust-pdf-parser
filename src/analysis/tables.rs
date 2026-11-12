//! Reconstruction of table structure.
//!
//! The pieces of text inside a table region are arranged into rows (by
//! baseline) and columns (by the vertical corridors of white space that run
//! through the table). Cells that cross a corridor become spanning cells,
//! labels set between rows become row-spanning cells, and rules are attached
//! to the cell edges they sit on.
//!
//! The reconstruction is deliberately conservative: when the content does not
//! look like a regular grid it returns `None` and the caller falls back to
//! other means of presenting the table.

use super::lines::Line;
use crate::common::geom::{Rect, median};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub struct GridCell {
    pub col: usize,
    pub colspan: usize,
    pub rowspan: usize,
    /// Pieces of text of the cell, left to right.
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone)]
pub struct Grid {
    pub bbox: Rect,
    pub n_cols: usize,
    /// `n_cols + 1` x positions.
    pub col_bounds: Vec<f32>,
    /// Horizontal extent of the text of each column.
    pub col_text: Vec<(f32, f32)>,
    pub col_align: Vec<Align>,
    /// `rows.len() + 1` y positions.
    pub row_bounds: Vec<f32>,
    pub row_baselines: Vec<f32>,
    pub rows: Vec<Vec<GridCell>>,
    /// Rule on the upper edge of each cell position: 0 = none, 1 = thin,
    /// 2 = heavy. Indexed `[row][col]`.
    pub rule_above: Vec<Vec<u8>>,
    /// Rule below the last row, per column.
    pub rule_bottom: Vec<u8>,
    /// Vertical rule on the left edge of each cell position.
    pub rule_left: Vec<Vec<bool>>,
    /// Vertical rule on the right edge of the table, per row.
    pub rule_right: Vec<bool>,
    pub header_rows: usize,
    /// Dominant font size of the table text.
    pub size: f32,
}

const BIN: f32 = 0.5;

/// Finds the vertical corridors within `[x0, x1]`: stretches crossed by at
/// most `tolerance` rows and bounded by text on both sides. Returns the x
/// position of a separator in each.
fn find_separators(rows: &[Vec<&Line>], x0: f32, x1: f32, tolerance: u16, min_bins: usize) -> Vec<f32> {
    let n_bins = ((x1 - x0) / BIN).ceil() as usize + 1;
    let mut crossing = vec![0u16; n_bins];
    for r in rows {
        let mut covered = vec![false; n_bins];
        for l in r {
            // Only pieces that lie inside the range count.
            if l.bbox.cx() < x0 || l.bbox.cx() > x1 {
                continue;
            }
            let a = ((l.bbox.x0 - x0) / BIN).floor().max(0.0) as usize;
            let b = (((l.bbox.x1 - x0) / BIN).ceil().max(0.0) as usize).min(n_bins - 1);
            if a <= b {
                for c in &mut covered[a..=b] {
                    *c = true;
                }
            }
        }
        for (i, c) in covered.iter().enumerate() {
            crossing[i] += *c as u16;
        }
    }
    let mut separators = Vec::new();
    let mut i = 0;
    while i < n_bins {
        if crossing[i] > tolerance {
            i += 1;
            continue;
        }
        let start = i;
        while i < n_bins && crossing[i] <= tolerance {
            i += 1;
        }
        if start == 0 || i >= n_bins || i - start < min_bins.max(1) {
            continue;
        }
        // Put the separator in the emptiest stretch of the corridor.
        let lo = crossing[start..i].iter().copied().min().unwrap();
        let (mut best, mut best_len, mut k) = (start, 0, start);
        while k < i {
            if crossing[k] == lo {
                let s = k;
                while k < i && crossing[k] == lo {
                    k += 1;
                }
                if k - s > best_len {
                    best = s;
                    best_len = k - s;
                }
            } else {
                k += 1;
            }
        }
        if best_len >= min_bins.max(1) {
            separators.push(x0 + (best as f32 + best_len as f32 * 0.5) * BIN);
        }
    }
    separators
}

/// Tries to arrange `lines` (the text inside `region`) into a grid.
pub fn extract_grid(region: Rect, lines: &[Line], hrules: &[(Rect, f32)], vrules: &[Rect]) -> Option<Grid> {
    let pieces: Vec<&Line> = lines.iter().filter(|l| !l.glyphs.is_empty()).collect();
    if pieces.len() < 4 {
        return None;
    }
    let sizes: Vec<f32> = pieces.iter().map(|l| l.size).collect();
    let size = median(&sizes, 9.0);
    // Tall delimiters, big operators: cells with two-dimensional maths.
    if pieces
        .iter()
        .any(|l| l.glyphs.iter().any(|g| g.ink.height() > 2.4 * l.size.max(size)))
    {
        return None;
    }

    // --- rows ---
    let mut order: Vec<&Line> = pieces.clone();
    order.sort_by(|a, b| a.baseline.total_cmp(&b.baseline));
    let mut rows: Vec<Vec<&Line>> = Vec::new();
    let mut row_y = f32::NEG_INFINITY;
    for l in order {
        if rows.is_empty() || l.baseline - row_y > 0.45 * size {
            rows.push(vec![l]);
            row_y = l.baseline;
        } else {
            rows.last_mut().unwrap().push(l);
        }
    }
    if rows.len() < 2 {
        return None;
    }
    for r in rows.iter_mut() {
        r.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
    }
    let baseline_of = |r: &Vec<&Line>| median(&r.iter().map(|l| l.baseline).collect::<Vec<_>>(), 0.0);

    // --- columns ---
    let x_min = pieces.iter().map(|l| l.bbox.x0).fold(f32::INFINITY, f32::min);
    let x_max = pieces.iter().map(|l| l.bbox.x1).fold(f32::NEG_INFINITY, f32::max);
    let min_bins = (0.4 * size / BIN) as usize;
    let n_all = rows.len();
    let tolerance = if n_all < 4 {
        0
    } else {
        ((n_all as f32 * 0.15) as u16).max(1)
    };
    let mut separators = find_separators(&rows, x_min, x_max, tolerance, min_bins);
    if separators.is_empty() {
        return None;
    }
    // A column whose rows mostly hold two pieces is really two columns with
    // a few cells spanning both; split it with a tolerance relative to the
    // rows that use the column.
    for _ in 0..3 {
        let mut bounds = vec![x_min - 1.0];
        bounds.extend(separators.iter().copied());
        bounds.push(x_max + 1.0);
        let mut added = Vec::new();
        for w in bounds.windows(2) {
            let (a, b) = (w[0], w[1]);
            let inside = |l: &&&Line| l.bbox.cx() >= a && l.bbox.cx() <= b;
            let with: usize = rows.iter().filter(|r| r.iter().any(|l| inside(&l))).count();
            let multi: usize = rows.iter().filter(|r| r.iter().filter(inside).count() >= 2).count();
            if with < 4 || multi < 2 || multi * 10 < with * 3 {
                continue;
            }
            let lo = rows
                .iter()
                .flatten()
                .filter(inside)
                .map(|l| l.bbox.x0)
                .fold(f32::INFINITY, f32::min);
            let hi = rows
                .iter()
                .flatten()
                .filter(inside)
                .map(|l| l.bbox.x1)
                .fold(f32::NEG_INFINITY, f32::max);
            let local = ((with as f32 * 0.34) as u16).max(1);
            for s in find_separators(&rows, lo, hi, local, min_bins) {
                // Both halves must be used by more rows than cross the split.
                let crossing = rows
                    .iter()
                    .filter(|r| r.iter().any(|l| inside(&l) && l.bbox.x0 < s && l.bbox.x1 > s))
                    .count();
                let left = rows
                    .iter()
                    .filter(|r| r.iter().any(|l| inside(&l) && l.bbox.x1 <= s))
                    .count();
                let right = rows
                    .iter()
                    .filter(|r| r.iter().any(|l| inside(&l) && l.bbox.x0 >= s))
                    .count();
                if left > crossing && right > crossing && left >= 2 && right >= 2 {
                    added.push(s);
                }
            }
        }
        if added.is_empty() {
            break;
        }
        separators.extend(added);
        separators.sort_by(|a, b| a.total_cmp(b));
    }
    let n_cols = separators.len() + 1;
    let col_of = |x: f32| separators.iter().filter(|&&s| s <= x).count();

    // --- cells ---
    let mut grid_rows: Vec<Vec<GridCell>> = Vec::with_capacity(rows.len());
    for r in &rows {
        let mut cells: Vec<GridCell> = Vec::new();
        for l in r {
            let c0 = col_of(l.bbox.x0 + 0.5);
            let c1 = col_of(l.bbox.x1 - 0.5).max(c0);
            match cells.last_mut() {
                // Another piece of the same cell.
                Some(prev) if c0 < prev.col + prev.colspan => {
                    prev.colspan = prev.colspan.max(c1 + 1 - prev.col);
                    prev.lines.push((*l).clone());
                }
                _ => cells.push(GridCell {
                    col: c0,
                    colspan: c1 - c0 + 1,
                    rowspan: 1,
                    lines: vec![(*l).clone()],
                }),
            }
        }
        grid_rows.push(cells);
    }
    let mut baselines: Vec<f32> = rows.iter().map(baseline_of).collect();

    // --- labels set between rows (vertically centred multi-row cells) ---
    let pitch = {
        let d: Vec<f32> = baselines.windows(2).map(|w| w[1] - w[0]).collect();
        median(&d, 1.2 * size)
    };
    let mut floating: Vec<(f32, GridCell)> = Vec::new();
    let mut k = 0;
    while k < grid_rows.len() {
        let lone = grid_rows[k].len() == 1 && grid_rows[k][0].colspan == 1;
        let near_prev = k > 0 && baselines[k] - baselines[k - 1] < 0.75 * pitch;
        let near_next = k + 1 < grid_rows.len() && baselines[k + 1] - baselines[k] < 0.75 * pitch;
        if lone && near_prev && near_next && grid_rows.len() > 2 {
            let cell = grid_rows.remove(k).pop().unwrap();
            floating.push((baselines.remove(k), cell));
        } else {
            k += 1;
        }
    }
    let n_rows = grid_rows.len();
    if n_rows < 2 {
        return None;
    }

    // --- rules ---
    let rule_x0 = hrules.iter().map(|r| r.0.x0).fold(f32::INFINITY, f32::min);
    let rule_x1 = hrules.iter().map(|r| r.0.x1).fold(f32::NEG_INFINITY, f32::max);
    let pad = 0.5 * size;
    let left_edge = if rule_x0.is_finite() {
        rule_x0.min(x_min)
    } else {
        x_min - pad
    };
    let right_edge = if rule_x1.is_finite() {
        rule_x1.max(x_max)
    } else {
        x_max + pad
    };
    let mut col_bounds = Vec::with_capacity(n_cols + 1);
    col_bounds.push(left_edge.max(region.x0 - 2.0));
    col_bounds.extend(separators.iter().copied());
    col_bounds.push(right_edge.min(region.x1 + 2.0));

    // Boundary k lies above row k; boundary n_rows lies below the last row.
    let mut row_bounds = Vec::with_capacity(n_rows + 1);
    row_bounds.push(baselines[0] - 1.05 * size);
    for k in 1..n_rows {
        row_bounds.push((baselines[k - 1] + baselines[k]) * 0.5 - 0.3 * size);
    }
    row_bounds.push(baselines[n_rows - 1] + 0.55 * size);

    let boundary_of = |y: f32| baselines.iter().filter(|&&b| b - 0.3 * size < y).count();
    let mut rule_above = vec![vec![0u8; n_cols]; n_rows];
    let mut rule_bottom = vec![0u8; n_cols];
    let mut full_rule_at = vec![false; n_rows + 1];
    let mut snapped: Vec<Option<f32>> = vec![None; n_rows + 1];
    for (r, lw) in hrules {
        let y = r.cy();
        let k = boundary_of(y);
        let weight = if *lw >= 0.7 { 2 } else { 1 };
        let mut covered = 0;
        for c in 0..n_cols {
            let (a, b) = (col_bounds[c], col_bounds[c + 1]);
            if r.x1.min(b) - r.x0.max(a) >= 0.5 * (b - a) {
                covered += 1;
                if k < n_rows {
                    rule_above[k][c] = rule_above[k][c].max(weight);
                } else {
                    rule_bottom[c] = rule_bottom[c].max(weight);
                }
            }
        }
        if covered * 10 >= n_cols * 9 {
            full_rule_at[k] = true;
        }
        if covered > 0 {
            snapped[k] = Some(y);
        }
    }
    // Put the row boundaries exactly on the rules where there are any.
    for k in 0..=n_rows {
        if let Some(y) = snapped[k] {
            let lo = if k == 0 {
                f32::NEG_INFINITY
            } else {
                baselines[k - 1] + 0.1 * size
            };
            let hi = if k == n_rows {
                f32::INFINITY
            } else {
                baselines[k] - 0.5 * size
            };
            if y > lo && y < hi {
                row_bounds[k] = y;
            }
        }
    }

    // Vertical rules: on the left edge of a column (or the right edge of
    // the table) for the rows they pass.
    let mut rule_left = vec![vec![false; n_cols]; n_rows];
    let mut rule_right = vec![false; n_rows];
    for v in vrules {
        let x = v.cx();
        // The column boundary nearest to the rule.
        let (j, dist) = col_bounds
            .iter()
            .enumerate()
            .map(|(j, b)| (j, (b - x).abs()))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        // Inner boundaries sit in the middle of a gap; accept a rule
        // anywhere in that gap.
        let gap_ok = j == 0 || j == n_cols || {
            let left_text = pieces.iter().filter(|l| l.bbox.x1 <= x + 0.5).count();
            let right_text = pieces.iter().filter(|l| l.bbox.x0 >= x - 0.5).count();
            left_text > 0 && right_text > 0 && dist < 4.0 * size
        };
        if !gap_ok {
            continue;
        }
        let mut any = false;
        for r in 0..n_rows {
            if baselines[r] >= v.y0 - 0.3 * size && baselines[r] - 0.5 * size <= v.y1 {
                any = true;
                if j < n_cols {
                    rule_left[r][j] = true;
                } else {
                    rule_right[r] = true;
                }
            }
        }
        if any && j > 0 && j < n_cols {
            col_bounds[j] = x;
        }
    }

    // --- place the floating labels as row-spanning cells ---
    for (y, mut cell) in floating {
        let c = cell.col;
        let pos = baselines.iter().filter(|&&b| b < y).count().clamp(1, n_rows - 1);
        let ruled = |k: usize| k == 0 || k == n_rows || rule_above[k][c] > 0;
        let g0 = (0..=pos).rev().find(|&k| ruled(k)).unwrap_or(0);
        let g1 = (pos..=n_rows).find(|&k| ruled(k)).unwrap_or(n_rows);
        let free = |r: usize| !grid_rows[r].iter().any(|x| x.col <= c && c < x.col + x.colspan);
        if g1 - g0 >= 2 && (g0..g1).all(free) {
            cell.rowspan = g1 - g0;
            grid_rows[g0].push(cell);
            grid_rows[g0].sort_by_key(|x| x.col);
        } else {
            // No clean group: attach to the nearest row with a free slot.
            let near = if pos < n_rows && (baselines[pos] - y).abs() < (y - baselines[pos - 1]).abs() {
                pos
            } else {
                pos - 1
            };
            let target = [near, pos.min(n_rows - 1), pos - 1].into_iter().find(|&r| free(r))?;
            grid_rows[target].push(cell);
            grid_rows[target].sort_by_key(|x| x.col);
        }
    }

    // --- plausibility ---
    let total_cells: usize = grid_rows.iter().map(|r| r.len()).sum();
    let spanning = grid_rows.iter().flatten().filter(|c| c.colspan > 1).count();
    let multi = grid_rows.iter().filter(|r| r.len() >= 2).count();
    if total_cells < 6 || multi * 10 < n_rows * 5 || spanning * 3 > total_cells {
        return None;
    }
    let mut used = vec![0usize; n_cols];
    for c in grid_rows.iter().flatten() {
        if c.colspan == 1 {
            used[c.col] += 1;
        }
    }
    if used.contains(&0) {
        return None;
    }

    // --- column geometry ---
    let mut col_text = vec![(f32::INFINITY, f32::NEG_INFINITY); n_cols];
    let mut samples: Vec<Vec<(f32, f32)>> = vec![Vec::new(); n_cols];
    for c in grid_rows.iter().flatten().filter(|c| c.colspan == 1) {
        let x0 = c.lines.iter().map(|l| l.bbox.x0).fold(f32::INFINITY, f32::min);
        let x1 = c.lines.iter().map(|l| l.bbox.x1).fold(f32::NEG_INFINITY, f32::max);
        col_text[c.col].0 = col_text[c.col].0.min(x0);
        col_text[c.col].1 = col_text[c.col].1.max(x1);
        samples[c.col].push((x0, x1));
    }
    let spread = |v: Vec<f32>| -> f32 {
        let lo = v.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        hi - lo
    };
    let col_align: Vec<Align> = samples
        .iter()
        .enumerate()
        .map(|(ci, s)| {
            if s.len() < 2 {
                return if ci == 0 { Align::Left } else { Align::Center };
            }
            let left = spread(s.iter().map(|p| p.0).collect());
            let right = spread(s.iter().map(|p| p.1).collect());
            let centre = spread(s.iter().map(|p| (p.0 + p.1) * 0.5).collect());
            if left <= centre + 0.3 && left <= right {
                Align::Left
            } else if right < centre - 0.3 {
                Align::Right
            } else {
                Align::Center
            }
        })
        .collect();

    // Header: the rows above the first full-width rule inside the table.
    let header_rows = (1..n_rows).find(|&k| full_rule_at[k]).filter(|&k| k <= 4).unwrap_or(0);

    let bbox = Rect::new(col_bounds[0], row_bounds[0], col_bounds[n_cols], row_bounds[n_rows]);
    Some(Grid {
        bbox,
        n_cols,
        col_bounds,
        col_text,
        col_align,
        row_bounds,
        row_baselines: baselines,
        rows: grid_rows,
        rule_above,
        rule_bottom,
        rule_left,
        rule_right,
        header_rows,
        size,
    })
}
