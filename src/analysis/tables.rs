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
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use crate::common::geom::{Rect, median};
use crate::model::Align;
use crate::model::Line;

/// Width (in points) of the bins in which the horizontal extent of a table
/// is sampled when looking for white-space corridors. Half a point is finer
/// than any gap that matters and keeps the bin count small.
const BIN_PT: f32 = 0.5;

/// Tuning parameters of table grid reconstruction. Part of [`LayoutConfig`](crate::config::LayoutConfig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TablesConfig {
    /// Regions with fewer pieces of text are not reconstructed: too little to
    /// tell a grid from a few labels.
    pub min_pieces_count: usize,
    /// Table font size (in points) assumed when no piece has a size; never
    /// used for a region that passed the piece count.
    pub default_size_pt: f32,
    /// A glyph taller than this (in font sizes) is a tall delimiter or big
    /// operator: the cells hold two-dimensional maths, which a grid of text
    /// lines cannot represent.
    pub max_glyph_height_em: f32,
    /// A piece whose baseline is more than this (in table font sizes) below
    /// the first baseline of the current row starts a new row.
    pub row_break_em: f32,
    /// Fewest rows of a grid, before and after the labels between rows are
    /// taken out.
    pub min_rows_count: usize,
    /// Narrowest white-space corridor (in table font sizes) that separates
    /// columns; a word space is about a quarter of the font size.
    pub min_corridor_em: f32,
    /// Tables with fewer rows than this need corridors that no row crosses;
    /// larger tables tolerate a few spanning cells.
    pub strict_corridor_below_rows_count: usize,
    /// In larger tables, the fraction of rows that may cross a corridor
    /// (spanning cells, long headers); at least one.
    pub corridor_crossing_ratio: f32,
    /// How many times columns are re-examined for hidden sub-columns; each
    /// pass can reveal a further split.
    pub column_split_passes_count: usize,
    /// A column is only re-examined for sub-columns when at least this many
    /// rows have text in it.
    pub split_min_rows_count: usize,
    /// ... and at least this many of those rows hold two or more pieces in
    /// it ...
    pub split_min_multi_rows_count: usize,
    /// ... and those rows are at least this many tenths of the rows using
    /// the column (integer tenths, to compare without rounding).
    pub split_min_multi_rows_tenths: usize,
    /// Fraction of the rows using a column that may cross a sub-column
    /// corridor; at least one.
    pub split_crossing_ratio: f32,
    /// Fewest rows that must use each side of a sub-column split.
    pub split_min_side_rows_count: usize,
    /// How far (in points) a piece's edges are moved inwards before finding
    /// the columns it covers, so that touching a separator is not crossing
    /// it.
    pub cell_edge_inset_pt: f32,
    /// Row pitch (in table font sizes) assumed when the table has a single
    /// row; 1.2 is the usual line spacing.
    pub default_pitch_em: f32,
    /// A lone cell whose baseline is closer than this fraction of the row
    /// pitch to both neighbouring rows is a label set between rows (a
    /// vertically centred multi-row cell).
    pub floating_label_max_pitch_ratio: f32,
    /// Without horizontal rules, how far (in table font sizes) the table
    /// extends beyond its leftmost and rightmost text.
    pub edge_pad_em: f32,
    /// How far (in points) the table may extend beyond the region it was
    /// found in.
    pub region_slack_pt: f32,
    /// Top of the first row above its baseline (in table font sizes): the
    /// ascender height plus a little space.
    pub first_row_top_em: f32,
    /// How far (in table font sizes) a boundary between rows is raised above
    /// the midpoint of their baselines, since text sits on its baseline and
    /// reaches up.
    pub row_boundary_raise_em: f32,
    /// Bottom of the last row below its baseline (in table font sizes): the
    /// descender depth plus a little space.
    pub last_row_bottom_em: f32,
    /// A horizontal rule lies above row `k` when it is above row `k`'s
    /// baseline minus this (in table font sizes).
    pub rule_row_offset_em: f32,
    /// Horizontal rules at least this thick (in points) are heavy (top and
    /// bottom rules of booktabs tables); thinner ones are thin.
    pub heavy_rule_min_width_pt: f32,
    /// A horizontal rule is on a column when it covers at least this
    /// fraction of the column's width.
    pub rule_column_cover_ratio: f32,
    /// A horizontal rule is full-width when it covers at least this many
    /// tenths of the columns (integer tenths, to compare without rounding).
    pub full_rule_min_cols_tenths: usize,
    /// A row boundary is moved onto a rule only when the rule is at least
    /// this far (in table font sizes) below the baseline of the row above...
    pub snap_min_below_em: f32,
    /// ... and at least this far (in table font sizes) above the baseline of
    /// the row below.
    pub snap_min_above_em: f32,
    /// Text counts as left (right) of a vertical rule when it ends (starts)
    /// within this many points past the rule.
    pub vrule_text_slack_pt: f32,
    /// An inner vertical rule may lie at most this far (in table font sizes)
    /// from the column boundary it is attached to.
    pub vrule_max_offset_em: f32,
    /// A vertical rule passes a row when it starts at most this far (in
    /// table font sizes) below the row's baseline...
    pub vrule_top_slack_em: f32,
    /// ... and ends at most this far (in table font sizes) above it.
    pub vrule_bottom_slack_em: f32,
    /// Grids with fewer cells are rejected as implausible.
    pub min_cells_count: usize,
    /// At least this many tenths of the rows must have two or more cells
    /// (integer tenths, to compare without rounding).
    pub min_multi_cell_rows_tenths: usize,
    /// At most one cell in this many may span several columns; more spanning
    /// cells mean the columns were not found correctly.
    pub spanning_cells_divisor: usize,
    /// Tolerance (in points) when comparing the spreads of left edges, right
    /// edges and centres of a column to choose its alignment.
    pub align_tolerance_pt: f32,
    /// The rows above the first full-width rule are header rows only if there
    /// are at most this many of them.
    pub max_header_rows_count: usize,
}

impl Default for TablesConfig {
    fn default() -> Self {
        Self {
            min_pieces_count: 4,
            default_size_pt: 9.0,
            max_glyph_height_em: 2.4,
            row_break_em: 0.45,
            min_rows_count: 2,
            min_corridor_em: 0.4,
            strict_corridor_below_rows_count: 4,
            corridor_crossing_ratio: 0.15,
            column_split_passes_count: 3,
            split_min_rows_count: 4,
            split_min_multi_rows_count: 2,
            split_min_multi_rows_tenths: 3,
            split_crossing_ratio: 0.34,
            split_min_side_rows_count: 2,
            cell_edge_inset_pt: 0.5,
            default_pitch_em: 1.2,
            floating_label_max_pitch_ratio: 0.75,
            edge_pad_em: 0.5,
            region_slack_pt: 2.0,
            first_row_top_em: 1.05,
            row_boundary_raise_em: 0.3,
            last_row_bottom_em: 0.55,
            rule_row_offset_em: 0.3,
            heavy_rule_min_width_pt: 0.7,
            rule_column_cover_ratio: 0.5,
            full_rule_min_cols_tenths: 9,
            snap_min_below_em: 0.1,
            snap_min_above_em: 0.5,
            vrule_text_slack_pt: 0.5,
            vrule_max_offset_em: 4.0,
            vrule_top_slack_em: 0.3,
            vrule_bottom_slack_em: 0.5,
            min_cells_count: 6,
            min_multi_cell_rows_tenths: 5,
            spanning_cells_divisor: 3,
            align_tolerance_pt: 0.3,
            max_header_rows_count: 4,
        }
    }
}

/// A cell of a reconstructed table.
#[derive(Debug, Clone)]
pub struct GridCell {
    /// First column of the cell.
    pub col: usize,
    /// Number of columns the cell spans (at least 1).
    pub colspan: usize,
    /// Number of rows the cell spans (at least 1).
    pub rowspan: usize,
    /// Pieces of text of the cell, left to right.
    pub lines: Vec<Line>,
}

/// A reconstructed table: rows of cells on a grid of column and row
/// boundaries, with the rules drawn on the grid's edges.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Box of the whole table (outer column and row boundaries).
    pub bbox: Rect,
    /// Number of columns.
    pub n_cols: usize,
    /// `n_cols + 1` x positions.
    pub col_bounds: Vec<f32>,
    /// Horizontal extent of the text of each column.
    pub col_text: Vec<(f32, f32)>,
    /// Alignment of the text in each column.
    pub col_align: Vec<Align>,
    /// `rows.len() + 1` y positions.
    pub row_bounds: Vec<f32>,
    /// Baseline of each row.
    pub row_baselines: Vec<f32>,
    /// The cells of each row, left to right. A row-spanning cell is listed in
    /// its first row only.
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
    /// Number of leading rows that form the table header (0 when unknown).
    pub header_rows: usize,
    /// Dominant font size of the table text.
    pub size: f32,
}

/// The text of a table region, grouped into rows.
struct TableText<'line> {
    /// The non-empty pieces of text.
    pieces: Vec<&'line Line>,
    /// The pieces grouped by baseline: rows top to bottom, pieces of a row
    /// left to right.
    rows: Vec<Vec<&'line Line>>,
    /// Median font size of the pieces.
    size: f32,
    /// Left edge of the leftmost piece.
    x_min: f32,
    /// Right edge of the rightmost piece.
    x_max: f32,
}

/// Where the horizontal rules of a table lie on its grid.
struct HorizontalRules {
    /// Rule weight above each cell position, `[row][col]`.
    rule_above: Vec<Vec<u8>>,
    /// Rule weight below the last row, per column.
    rule_bottom: Vec<u8>,
    /// For each row boundary, whether a rule covers (nearly) all columns.
    full_rule_at: Vec<bool>,
    /// For each row boundary, the y position of a rule on it, if any.
    snapped: Vec<Option<f32>>,
}

/// Vertical rules of a table, per row.
struct VerticalRules {
    /// Rule on the left edge of each cell position, `[row][col]`.
    rule_left: Vec<Vec<bool>>,
    /// Rule on the right edge of the table, per row.
    rule_right: Vec<bool>,
}

/// Cell counts used by the plausibility rules.
struct CellCounts {
    /// Number of cells.
    total: usize,
    /// Number of cells that span several columns.
    spanning: usize,
    /// Number of rows with at least two cells.
    multi_cell_rows: usize,
    /// Number of rows.
    rows: usize,
}

/// Tries to arrange `lines` (the text inside `region`) into a grid.
///
/// `hrules` are the horizontal rules of the region with their stroke widths,
/// `vrules` its vertical rules. Returns `None` when the text does not form a
/// plausible grid: too few pieces or rows, two-dimensional maths in the
/// cells, no column corridor, a label that fits no row, or a result that
/// fails the plausibility rules.
pub fn extract_grid(
    region: Rect,
    lines: &[Line],
    hrules: &[(Rect, f32)],
    vrules: &[Rect],
    cfg: &TablesConfig,
) -> Option<Grid> {
    let text = table_text(lines, cfg)?;
    let size = text.size;
    let separators = find_columns(&text, cfg)?;
    let n_cols = separators.len() + 1;
    let mut grid_rows = build_cells(&text.rows, &separators, cfg);
    let mut baselines: Vec<f32> = text.rows.iter().map(|row| row_baseline(row)).collect();
    let floating = take_floating_labels(&mut grid_rows, &mut baselines, size, cfg);
    let n_rows = grid_rows.len();
    if n_rows < cfg.min_rows_count {
        return None;
    }

    let mut col_bounds = column_bounds(region, hrules, &separators, &text, cfg);
    let mut row_bounds = row_bounds(&baselines, size, cfg);
    let horizontal = horizontal_rules(hrules, &col_bounds, &baselines, size, cfg);
    snap_row_bounds(&mut row_bounds, &horizontal.snapped, &baselines, size, cfg);
    let vertical = vertical_rules(vrules, &text.pieces, &mut col_bounds, &baselines, size, cfg);
    place_floating_labels(floating, &mut grid_rows, &baselines, &horizontal.rule_above)?;

    if !is_plausible(&grid_rows, n_cols, cfg) {
        return None;
    }
    let (col_text, col_align) = column_geometry(&grid_rows, n_cols, cfg);

    // Header: the rows above the first full-width rule inside the table.
    let header_rows = (1..n_rows)
        .find(|&k| horizontal.full_rule_at[k])
        .filter(|&k| k <= cfg.max_header_rows_count)
        .unwrap_or(0);

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
        rule_above: horizontal.rule_above,
        rule_bottom: horizontal.rule_bottom,
        rule_left: vertical.rule_left,
        rule_right: vertical.rule_right,
        header_rows,
        size,
    })
}

/// Collects the non-empty pieces of the region, measures their size and
/// groups them into rows. `None` when there are too few pieces or rows, or
/// when a cell holds two-dimensional maths.
fn table_text<'line>(lines: &'line [Line], cfg: &TablesConfig) -> Option<TableText<'line>> {
    let pieces: Vec<&Line> = lines.iter().filter(|line| !line.glyphs.is_empty()).collect();
    if pieces.len() < cfg.min_pieces_count {
        return None;
    }
    let sizes: Vec<f32> = pieces.iter().map(|line| line.size).collect();
    let size = median(&sizes, cfg.default_size_pt);
    // Tall delimiters, big operators: cells with two-dimensional maths.
    if pieces.iter().any(|line| {
        line.glyphs
            .iter()
            .any(|glyph| glyph.ink.height() > cfg.max_glyph_height_em * line.size.max(size))
    }) {
        return None;
    }
    let rows = group_rows(&pieces, size, cfg)?;
    let x_min = pieces.iter().map(|line| line.bbox.x0).fold(f32::INFINITY, f32::min);
    let x_max = pieces.iter().map(|line| line.bbox.x1).fold(f32::NEG_INFINITY, f32::max);
    Some(TableText {
        pieces,
        rows,
        size,
        x_min,
        x_max,
    })
}

/// Groups the pieces into rows by baseline (top to bottom), each row sorted
/// left to right. `None` when there are fewer rows than a grid needs.
fn group_rows<'line>(pieces: &[&'line Line], size: f32, cfg: &TablesConfig) -> Option<Vec<Vec<&'line Line>>> {
    let mut order: Vec<&Line> = pieces.to_vec();
    order.sort_by(|a, b| a.baseline.total_cmp(&b.baseline));
    let mut rows: Vec<Vec<&Line>> = Vec::new();
    let mut row_y = f32::NEG_INFINITY;
    for line in order {
        if rows.is_empty() || line.baseline - row_y > cfg.row_break_em * size {
            rows.push(vec![line]);
            row_y = line.baseline;
        } else {
            rows.last_mut().unwrap().push(line);
        }
    }
    if rows.len() < cfg.min_rows_count {
        return None;
    }
    for row in rows.iter_mut() {
        row.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
    }
    Some(rows)
}

/// Median baseline of the pieces of a row.
fn row_baseline(row: &[&Line]) -> f32 {
    median(&row.iter().map(|line| line.baseline).collect::<Vec<_>>(), 0.0)
}

/// Finds the column separators (x positions, left to right): corridors
/// through the whole table, then corridors inside columns that turn out to
/// be two columns with a few spanning cells. `None` when there is no
/// corridor at all.
fn find_columns(text: &TableText, cfg: &TablesConfig) -> Option<Vec<f32>> {
    let rows = &text.rows;
    let min_bins = (cfg.min_corridor_em * text.size / BIN_PT) as usize;
    let n_all = rows.len();
    let tolerance = if n_all < cfg.strict_corridor_below_rows_count {
        0
    } else {
        ((n_all as f32 * cfg.corridor_crossing_ratio) as u16).max(1)
    };
    let mut separators = find_separators(rows, text.x_min, text.x_max, tolerance, min_bins);
    if separators.is_empty() {
        return None;
    }
    // A column whose rows mostly hold two pieces is really two columns with
    // a few cells spanning both; split it with a tolerance relative to the
    // rows that use the column.
    for _ in 0..cfg.column_split_passes_count {
        let mut bounds = vec![text.x_min - 1.0];
        bounds.extend(separators.iter().copied());
        bounds.push(text.x_max + 1.0);
        let mut added = Vec::new();
        for pair in bounds.windows(2) {
            split_column(rows, pair[0], pair[1], min_bins, cfg, &mut added);
        }
        if added.is_empty() {
            break;
        }
        separators.extend(added);
        separators.sort_by(|a, b| a.total_cmp(b));
    }
    Some(separators)
}

/// Looks for sub-column separators inside the column between `left` and
/// `right` and appends those that both halves support to `added`. Columns
/// used by too few rows, or whose rows rarely hold two pieces, are left
/// alone.
fn split_column(rows: &[Vec<&Line>], left: f32, right: f32, min_bins: usize, cfg: &TablesConfig, added: &mut Vec<f32>) {
    let inside = |line: &&&Line| line.bbox.cx() >= left && line.bbox.cx() <= right;
    let with: usize = rows.iter().filter(|row| row.iter().any(|line| inside(&line))).count();
    let multi: usize = rows.iter().filter(|row| row.iter().filter(inside).count() >= 2).count();
    if with < cfg.split_min_rows_count
        || multi < cfg.split_min_multi_rows_count
        || multi * 10 < with * cfg.split_min_multi_rows_tenths
    {
        return;
    }
    let lo = rows
        .iter()
        .flatten()
        .filter(inside)
        .map(|line| line.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    let hi = rows
        .iter()
        .flatten()
        .filter(inside)
        .map(|line| line.bbox.x1)
        .fold(f32::NEG_INFINITY, f32::max);
    let local = ((with as f32 * cfg.split_crossing_ratio) as u16).max(1);
    for split in find_separators(rows, lo, hi, local, min_bins) {
        // Both halves must be used by more rows than cross the split.
        let crossing = rows
            .iter()
            .filter(|row| {
                row.iter()
                    .any(|line| inside(&line) && line.bbox.x0 < split && line.bbox.x1 > split)
            })
            .count();
        let left_rows = rows
            .iter()
            .filter(|row| row.iter().any(|line| inside(&line) && line.bbox.x1 <= split))
            .count();
        let right_rows = rows
            .iter()
            .filter(|row| row.iter().any(|line| inside(&line) && line.bbox.x0 >= split))
            .count();
        if left_rows > crossing
            && right_rows > crossing
            && left_rows >= cfg.split_min_side_rows_count
            && right_rows >= cfg.split_min_side_rows_count
        {
            added.push(split);
        }
    }
}

/// Finds the vertical corridors within `[x0, x1]`: stretches crossed by at
/// most `tolerance` rows and bounded by text on both sides. Returns the x
/// position of a separator in each.
///
/// Only pieces whose centre lies inside the range count. A corridor must be
/// at least `min_bins` bins (of [`BIN_PT`]) wide, and so must its emptiest
/// stretch, where the separator is put.
fn find_separators(rows: &[Vec<&Line>], x0: f32, x1: f32, tolerance: u16, min_bins: usize) -> Vec<f32> {
    let crossing = crossing_counts(rows, x0, x1);
    let n_bins = crossing.len();
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
        let (best, best_len) = emptiest_stretch(&crossing, start, i);
        if best_len >= min_bins.max(1) {
            separators.push(x0 + (best as f32 + best_len as f32 * 0.5) * BIN_PT);
        }
    }
    separators
}

/// For each bin of `[x0, x1]`, the number of rows with text in it.
fn crossing_counts(rows: &[Vec<&Line>], x0: f32, x1: f32) -> Vec<u16> {
    let n_bins = ((x1 - x0) / BIN_PT).ceil() as usize + 1;
    let mut crossing = vec![0u16; n_bins];
    for row in rows {
        let mut covered = vec![false; n_bins];
        for line in row {
            // Only pieces that lie inside the range count.
            if line.bbox.cx() < x0 || line.bbox.cx() > x1 {
                continue;
            }
            let first_bin = ((line.bbox.x0 - x0) / BIN_PT).floor().max(0.0) as usize;
            let last_bin = (((line.bbox.x1 - x0) / BIN_PT).ceil().max(0.0) as usize).min(n_bins - 1);
            if first_bin <= last_bin {
                for flag in &mut covered[first_bin..=last_bin] {
                    *flag = true;
                }
            }
        }
        for (i, flag) in covered.iter().enumerate() {
            crossing[i] += *flag as u16;
        }
    }
    crossing
}

/// The longest run of bins in `crossing[start..end]` with the smallest
/// crossing count, as `(first bin, length)`; the leftmost run on ties.
fn emptiest_stretch(crossing: &[u16], start: usize, end: usize) -> (usize, usize) {
    let lowest = crossing[start..end].iter().copied().min().unwrap();
    let (mut best, mut best_len, mut k) = (start, 0, start);
    while k < end {
        if crossing[k] == lowest {
            let run_start = k;
            while k < end && crossing[k] == lowest {
                k += 1;
            }
            if k - run_start > best_len {
                best = run_start;
                best_len = k - run_start;
            }
        } else {
            k += 1;
        }
    }
    (best, best_len)
}

/// Turns the rows of pieces into rows of cells. Pieces that fall into the
/// columns of the previous cell join it; a piece crossing separators makes
/// its cell span columns.
fn build_cells(rows: &[Vec<&Line>], separators: &[f32], cfg: &TablesConfig) -> Vec<Vec<GridCell>> {
    let col_of = |x: f32| separators.iter().filter(|&&separator| separator <= x).count();
    let mut grid_rows: Vec<Vec<GridCell>> = Vec::with_capacity(rows.len());
    for row in rows {
        let mut cells: Vec<GridCell> = Vec::new();
        for line in row {
            let first_col = col_of(line.bbox.x0 + cfg.cell_edge_inset_pt);
            let last_col = col_of(line.bbox.x1 - cfg.cell_edge_inset_pt).max(first_col);
            match cells.last_mut() {
                // Another piece of the same cell.
                Some(prev) if first_col < prev.col + prev.colspan => {
                    prev.colspan = prev.colspan.max(last_col + 1 - prev.col);
                    prev.lines.push((*line).clone());
                }
                _ => cells.push(GridCell {
                    col: first_col,
                    colspan: last_col - first_col + 1,
                    rowspan: 1,
                    lines: vec![(*line).clone()],
                }),
            }
        }
        grid_rows.push(cells);
    }
    grid_rows
}

/// Labels set between rows (vertically centred multi-row cells): removes
/// rows made of one single-column cell that sit much closer than the row
/// pitch to both neighbours, and returns them with their baselines.
fn take_floating_labels(
    grid_rows: &mut Vec<Vec<GridCell>>,
    baselines: &mut Vec<f32>,
    size: f32,
    cfg: &TablesConfig,
) -> Vec<(f32, GridCell)> {
    let pitch = {
        let gaps: Vec<f32> = baselines.windows(2).map(|pair| pair[1] - pair[0]).collect();
        median(&gaps, cfg.default_pitch_em * size)
    };
    let max_distance = cfg.floating_label_max_pitch_ratio * pitch;
    let mut floating: Vec<(f32, GridCell)> = Vec::new();
    let mut k = 0;
    while k < grid_rows.len() {
        let lone = grid_rows[k].len() == 1 && grid_rows[k][0].colspan == 1;
        let near_prev = k > 0 && baselines[k] - baselines[k - 1] < max_distance;
        let near_next = k + 1 < grid_rows.len() && baselines[k + 1] - baselines[k] < max_distance;
        if lone && near_prev && near_next && grid_rows.len() > 2 {
            let cell = grid_rows.remove(k).pop().unwrap();
            floating.push((baselines.remove(k), cell));
        } else {
            k += 1;
        }
    }
    floating
}

/// The `n_cols + 1` column boundaries: the outer edges of the rules (or of
/// the text, padded, when there are no rules), kept close to the region,
/// with the separators in between.
fn column_bounds(
    region: Rect,
    hrules: &[(Rect, f32)],
    separators: &[f32],
    text: &TableText,
    cfg: &TablesConfig,
) -> Vec<f32> {
    let rule_x0 = hrules.iter().map(|rule| rule.0.x0).fold(f32::INFINITY, f32::min);
    let rule_x1 = hrules.iter().map(|rule| rule.0.x1).fold(f32::NEG_INFINITY, f32::max);
    let pad = cfg.edge_pad_em * text.size;
    let left_edge = if rule_x0.is_finite() {
        rule_x0.min(text.x_min)
    } else {
        text.x_min - pad
    };
    let right_edge = if rule_x1.is_finite() {
        rule_x1.max(text.x_max)
    } else {
        text.x_max + pad
    };
    let mut col_bounds = Vec::with_capacity(separators.len() + 2);
    col_bounds.push(left_edge.max(region.x0 - cfg.region_slack_pt));
    col_bounds.extend(separators.iter().copied());
    col_bounds.push(right_edge.min(region.x1 + cfg.region_slack_pt));
    col_bounds
}

/// The `n_rows + 1` row boundaries derived from the baselines. Boundary `k`
/// lies above row `k`; boundary `n_rows` lies below the last row.
fn row_bounds(baselines: &[f32], size: f32, cfg: &TablesConfig) -> Vec<f32> {
    let n_rows = baselines.len();
    let mut row_bounds = Vec::with_capacity(n_rows + 1);
    row_bounds.push(baselines[0] - cfg.first_row_top_em * size);
    for k in 1..n_rows {
        row_bounds.push((baselines[k - 1] + baselines[k]) * 0.5 - cfg.row_boundary_raise_em * size);
    }
    row_bounds.push(baselines[n_rows - 1] + cfg.last_row_bottom_em * size);
    row_bounds
}

/// Attaches each horizontal rule to the row boundary it lies on and to the
/// columns it covers, and records where full-width rules are.
fn horizontal_rules(
    hrules: &[(Rect, f32)],
    col_bounds: &[f32],
    baselines: &[f32],
    size: f32,
    cfg: &TablesConfig,
) -> HorizontalRules {
    let n_rows = baselines.len();
    let n_cols = col_bounds.len() - 1;
    let boundary_of = |y: f32| {
        baselines
            .iter()
            .filter(|&&baseline| baseline - cfg.rule_row_offset_em * size < y)
            .count()
    };
    let mut rules = HorizontalRules {
        rule_above: vec![vec![0u8; n_cols]; n_rows],
        rule_bottom: vec![0u8; n_cols],
        full_rule_at: vec![false; n_rows + 1],
        snapped: vec![None; n_rows + 1],
    };
    for (rule, stroke_width) in hrules {
        let y = rule.cy();
        let k = boundary_of(y);
        let weight = if *stroke_width >= cfg.heavy_rule_min_width_pt {
            2
        } else {
            1
        };
        let mut covered = 0;
        for col in 0..n_cols {
            let (col_x0, col_x1) = (col_bounds[col], col_bounds[col + 1]);
            if rule.x1.min(col_x1) - rule.x0.max(col_x0) >= cfg.rule_column_cover_ratio * (col_x1 - col_x0) {
                covered += 1;
                if k < n_rows {
                    rules.rule_above[k][col] = rules.rule_above[k][col].max(weight);
                } else {
                    rules.rule_bottom[col] = rules.rule_bottom[col].max(weight);
                }
            }
        }
        if covered * 10 >= n_cols * cfg.full_rule_min_cols_tenths {
            rules.full_rule_at[k] = true;
        }
        if covered > 0 {
            rules.snapped[k] = Some(y);
        }
    }
    rules
}

/// Puts the row boundaries exactly on the rules where there are any, as long
/// as the rule lies clearly between the rows' text.
fn snap_row_bounds(row_bounds: &mut [f32], snapped: &[Option<f32>], baselines: &[f32], size: f32, cfg: &TablesConfig) {
    let n_rows = baselines.len();
    for k in 0..=n_rows {
        if let Some(y) = snapped[k] {
            let lo = if k == 0 {
                f32::NEG_INFINITY
            } else {
                baselines[k - 1] + cfg.snap_min_below_em * size
            };
            let hi = if k == n_rows {
                f32::INFINITY
            } else {
                baselines[k] - cfg.snap_min_above_em * size
            };
            if y > lo && y < hi {
                row_bounds[k] = y;
            }
        }
    }
}

/// Vertical rules: on the left edge of a column (or the right edge of the
/// table) for the rows they pass. An inner column boundary with a rule is
/// moved onto the rule.
fn vertical_rules(
    vrules: &[Rect],
    pieces: &[&Line],
    col_bounds: &mut [f32],
    baselines: &[f32],
    size: f32,
    cfg: &TablesConfig,
) -> VerticalRules {
    let n_rows = baselines.len();
    let n_cols = col_bounds.len() - 1;
    let mut rules = VerticalRules {
        rule_left: vec![vec![false; n_cols]; n_rows],
        rule_right: vec![false; n_rows],
    };
    for vrule in vrules {
        let x = vrule.cx();
        // The column boundary nearest to the rule.
        let (j, dist) = col_bounds
            .iter()
            .enumerate()
            .map(|(j, bound)| (j, (bound - x).abs()))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        // Inner boundaries sit in the middle of a gap; accept a rule
        // anywhere in that gap.
        let gap_ok = j == 0 || j == n_cols || {
            let left_text = pieces
                .iter()
                .filter(|line| line.bbox.x1 <= x + cfg.vrule_text_slack_pt)
                .count();
            let right_text = pieces
                .iter()
                .filter(|line| line.bbox.x0 >= x - cfg.vrule_text_slack_pt)
                .count();
            left_text > 0 && right_text > 0 && dist < cfg.vrule_max_offset_em * size
        };
        if !gap_ok {
            continue;
        }
        let mut any = false;
        for (row, &baseline) in baselines.iter().enumerate() {
            if baseline >= vrule.y0 - cfg.vrule_top_slack_em * size
                && baseline - cfg.vrule_bottom_slack_em * size <= vrule.y1
            {
                any = true;
                if j < n_cols {
                    rules.rule_left[row][j] = true;
                } else {
                    rules.rule_right[row] = true;
                }
            }
        }
        if any && j > 0 && j < n_cols {
            col_bounds[j] = x;
        }
    }
    rules
}

/// Places the labels set between rows as row-spanning cells: over the group
/// of rows between the nearest rules around the label when that group is
/// free in the label's column, otherwise in the nearest row with a free
/// slot. `None` when no row has a free slot for a label.
fn place_floating_labels(
    floating: Vec<(f32, GridCell)>,
    grid_rows: &mut [Vec<GridCell>],
    baselines: &[f32],
    rule_above: &[Vec<u8>],
) -> Option<()> {
    let n_rows = grid_rows.len();
    for (y, mut cell) in floating {
        let col = cell.col;
        let pos = baselines
            .iter()
            .filter(|&&baseline| baseline < y)
            .count()
            .clamp(1, n_rows - 1);
        let ruled = |k: usize| k == 0 || k == n_rows || rule_above[k][col] > 0;
        let group_start = (0..=pos).rev().find(|&k| ruled(k)).unwrap_or(0);
        let group_end = (pos..=n_rows).find(|&k| ruled(k)).unwrap_or(n_rows);
        let free = |row: usize| {
            !grid_rows[row]
                .iter()
                .any(|other| other.col <= col && col < other.col + other.colspan)
        };
        if group_end - group_start >= 2 && (group_start..group_end).all(free) {
            cell.rowspan = group_end - group_start;
            grid_rows[group_start].push(cell);
            grid_rows[group_start].sort_by_key(|other| other.col);
        } else {
            // No clean group: attach to the nearest row with a free slot.
            let near = if pos < n_rows && (baselines[pos] - y).abs() < (y - baselines[pos - 1]).abs() {
                pos
            } else {
                pos - 1
            };
            let target = [near, pos.min(n_rows - 1), pos - 1]
                .into_iter()
                .find(|&row| free(row))?;
            grid_rows[target].push(cell);
            grid_rows[target].sort_by_key(|other| other.col);
        }
    }
    Some(())
}

/// Whether the reconstructed grid looks like a real table: the plausibility
/// rules, evaluated in order until one fails.
fn is_plausible(grid_rows: &[Vec<GridCell>], n_cols: usize, cfg: &TablesConfig) -> bool {
    let counts = CellCounts {
        total: grid_rows.iter().map(|row| row.len()).sum(),
        spanning: grid_rows.iter().flatten().filter(|cell| cell.colspan > 1).count(),
        multi_cell_rows: grid_rows.iter().filter(|row| row.len() >= 2).count(),
        rows: grid_rows.len(),
    };
    has_enough_cells(&counts, cfg)
        && has_enough_multi_cell_rows(&counts, cfg)
        && has_few_spanning_cells(&counts, cfg)
        && every_column_used(grid_rows, n_cols)
}

/// Rule: the grid has a minimum number of cells.
fn has_enough_cells(counts: &CellCounts, cfg: &TablesConfig) -> bool {
    counts.total >= cfg.min_cells_count
}

/// Rule: enough rows have two or more cells; a list of single cells is not
/// a table.
fn has_enough_multi_cell_rows(counts: &CellCounts, cfg: &TablesConfig) -> bool {
    counts.multi_cell_rows * 10 >= counts.rows * cfg.min_multi_cell_rows_tenths
}

/// Rule: only a small share of the cells span several columns.
fn has_few_spanning_cells(counts: &CellCounts, cfg: &TablesConfig) -> bool {
    counts.spanning * cfg.spanning_cells_divisor <= counts.total
}

/// Rule: every column holds at least one single-column cell; otherwise a
/// separator was put inside cells.
fn every_column_used(grid_rows: &[Vec<GridCell>], n_cols: usize) -> bool {
    let mut used = vec![0usize; n_cols];
    for cell in grid_rows.iter().flatten() {
        if cell.colspan == 1 {
            used[cell.col] += 1;
        }
    }
    !used.contains(&0)
}

/// The horizontal extent of the text of each column and the column's
/// alignment, both measured on the single-column cells.
fn column_geometry(grid_rows: &[Vec<GridCell>], n_cols: usize, cfg: &TablesConfig) -> (Vec<(f32, f32)>, Vec<Align>) {
    let mut col_text = vec![(f32::INFINITY, f32::NEG_INFINITY); n_cols];
    let mut samples: Vec<Vec<(f32, f32)>> = vec![Vec::new(); n_cols];
    for cell in grid_rows.iter().flatten().filter(|cell| cell.colspan == 1) {
        let x0 = cell.lines.iter().map(|line| line.bbox.x0).fold(f32::INFINITY, f32::min);
        let x1 = cell
            .lines
            .iter()
            .map(|line| line.bbox.x1)
            .fold(f32::NEG_INFINITY, f32::max);
        col_text[cell.col].0 = col_text[cell.col].0.min(x0);
        col_text[cell.col].1 = col_text[cell.col].1.max(x1);
        samples[cell.col].push((x0, x1));
    }
    let col_align: Vec<Align> = samples
        .iter()
        .enumerate()
        .map(|(col, extents)| column_align(col, extents, cfg))
        .collect();
    (col_text, col_align)
}

/// Alignment of a column from the `(left, right)` extents of its cells: the
/// edge (or centre) that varies least. With fewer than two cells the first
/// column is left-aligned and the others centred.
fn column_align(col: usize, extents: &[(f32, f32)], cfg: &TablesConfig) -> Align {
    if extents.len() < 2 {
        return if col == 0 { Align::Left } else { Align::Center };
    }
    let left = spread(extents.iter().map(|extent| extent.0).collect());
    let right = spread(extents.iter().map(|extent| extent.1).collect());
    let centre = spread(extents.iter().map(|extent| (extent.0 + extent.1) * 0.5).collect());
    if left <= centre + cfg.align_tolerance_pt && left <= right {
        Align::Left
    } else if right < centre - cfg.align_tolerance_pt {
        Align::Right
    } else {
        Align::Center
    }
}

/// Difference between the largest and the smallest value.
fn spread(values: Vec<f32>) -> f32 {
    let lo = values.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    hi - lo
}
