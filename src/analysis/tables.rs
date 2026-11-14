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

use crate::analysis::lines::{LinesConfig, split_at_positions};
use crate::common::geom::{Rect, median};
use crate::model::Align;
use crate::model::Line;

/// Width (in points) of the bins in which the horizontal extent of a table
/// is sampled when looking for white-space corridors. Half a point is finer
/// than any gap that matters and keeps the bin count small.
const BIN_PT: f32 = 0.5;

/// Letters that follow a number as its unit or multiplier ("125.0M", "1.7B",
/// "473K", "2x") without making it a word.
const NUMBER_UNITS: &str = "MKBGkx";

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
    /// A horizontal rule separates the logical rows of a table with
    /// multi-line cells when it covers at least this fraction of the width of
    /// the table's text.
    pub band_rule_min_ratio: f32,
    /// Without a rule between them, two lines belong to one multi-line cell
    /// when their baselines are at most this many times the table's tightest
    /// line pitch apart.
    pub continuation_gap_ratio: f32,
    /// A table has multi-line cells when its logical rows (cut by rules or
    /// by loose gaps) number at least this many ...
    pub band_min_count: usize,
    /// ... and at least this many tenths of them hold several lines
    /// (integer tenths, to compare without rounding) ...
    pub multiline_bands_tenths: usize,
    /// ... and none holds more lines than this.
    pub band_max_lines_count: usize,
    /// A grid with at least this many cells made of several figures side by
    /// side is not trusted: two columns of numbers were taken for one.
    pub merged_figure_cells_min_count: usize,
    /// A table is cut into logical rows by its rules alone when at least this
    /// many of them run between lines of text; with fewer, the gaps between
    /// lines decide as well.
    pub band_rules_min_count: usize,
    /// A line is the continuation of the one above it when it is at most this
    /// many font sizes below ...
    pub continuation_max_dy_em: f32,
    /// ... and at least this many such lines must start with a lowercase
    /// letter ...
    pub continuation_min_lowercase_count: usize,
    /// ... and at least this many tenths of them (integer tenths, to
    /// compare without rounding).
    pub continuation_lowercase_tenths: usize,
    /// Narrowest white-space corridor (in table font sizes) that separates
    /// columns; a word space is about a quarter of the font size.
    pub min_corridor_em: f32,
    /// A corridor tolerating a few crossing rows is put where it is clear in
    /// every row, and that clear stretch must be at least this wide (in table
    /// font sizes): headers set over the columns may crowd the gap.
    pub min_clear_corridor_em: f32,
    /// A corridor between two columns of numbers may be this narrow (in table
    /// font sizes): numbers are set tightly, and a gap of this width between
    /// two numbers is never a word space.
    pub min_numeric_corridor_em: f32,
    /// A narrow corridor counts as one between numbers when, in at least
    /// this many rows, the words on both sides of it are numbers, and in
    /// most of the rows that have words on both sides.
    pub numeric_corridor_min_rows_count: usize,
    /// A column is split where it has a corridor that all the rows with two
    /// or more pieces in the column share, even when other rows (spanning
    /// cells) cross it; this many such rows are needed, and at least this
    /// many must have pieces on both sides.
    pub multi_row_split_min_rows_count: usize,
    /// A piece that separators run through is split into its words only when
    /// each word is lined up with the words of other rows: its left edge or
    /// its right edge agrees to within this many points with that of ...
    pub aligned_word_tolerance_pt: f32,
    /// ... at least this many of them (a column of cells, not a chance).
    pub aligned_words_min_count: usize,
    /// ... or the piece has at least this many words, one per column it
    /// crosses: a row of labels, which need not line up with anything.
    pub split_unaligned_min_words_count: usize,
    /// A row is a row of numbers when at least this many tenths of its words
    /// are numbers (integer tenths, to compare without rounding) ...
    pub numeric_row_min_tenths: usize,
    /// ... and tables with at least this many such rows get the corridors
    /// between their numbers looked for apart from the header rows and labels
    /// that cross them.
    pub numeric_rivers_min_rows_count: usize,
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
            band_rule_min_ratio: 0.5,
            continuation_gap_ratio: 1.35,
            band_min_count: 3,
            multiline_bands_tenths: 5,
            band_max_lines_count: 12,
            merged_figure_cells_min_count: 2,
            band_rules_min_count: 3,
            continuation_max_dy_em: 1.6,
            continuation_min_lowercase_count: 2,
            continuation_lowercase_tenths: 4,
            min_corridor_em: 0.4,
            min_clear_corridor_em: 0.1,
            min_numeric_corridor_em: 0.15,
            numeric_corridor_min_rows_count: 2,
            multi_row_split_min_rows_count: 2,
            aligned_word_tolerance_pt: 0.6,
            aligned_words_min_count: 2,
            split_unaligned_min_words_count: 4,
            numeric_row_min_tenths: 8,
            numeric_rivers_min_rows_count: 3,
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
    /// left to right. In a table with multi-line cells a row is a logical
    /// row: all the lines of its cells, top to bottom.
    rows: Vec<Vec<&'line Line>>,
    /// Whether the rows are logical rows of multi-line cells (see
    /// [`merge_continuation_rows`]).
    multiline: bool,
    /// Median font size of the pieces.
    size: f32,
    /// Left edge of the leftmost piece.
    x_min: f32,
    /// Right edge of the rightmost piece.
    x_max: f32,
}

/// How wide (in bins of [`BIN_PT`]) a corridor between columns must be.
#[derive(Clone, Copy)]
struct CorridorBins {
    /// The stretch crossed by at most the tolerated number of rows.
    run: usize,
    /// Its part that no row crosses, where the separator is put.
    clear: usize,
    /// The narrowest stretch that separates two columns of numbers.
    numeric_run: usize,
    /// Rows with numbers on both sides that make a stretch numeric.
    numeric_rows: usize,
    /// Text is taken word by word (not piece by piece) when finding
    /// corridors.
    words: bool,
}

/// A piece of a row of cells, as it goes into the grid.
struct RowPiece {
    /// The text.
    line: Line,
    /// The piece is the label of one column that is not lined up with the
    /// column's numbers: it belongs to the column its centre is in, however
    /// far its text reaches.
    centred: bool,
}

/// A vertical corridor between columns.
struct Corridor {
    /// Left edge of the stretch crossed by few enough rows.
    start: f32,
    /// Right edge of that stretch.
    end: f32,
    /// Where the separator is put: the middle of the stretch's emptiest part.
    at: f32,
}

/// A word of a piece of text: the glyphs between two word spaces.
struct Word {
    /// Left edge.
    x0: f32,
    /// Right edge.
    x1: f32,
    /// The word is a number (digits, signs, a percent sign, an exponent).
    numeric: bool,
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
///
/// Cells wrapped over several lines are read as one cell when the table
/// shows the signs of it (see [`merge_continuation_rows`]); when that does
/// not give a plausible grid, the lines are tried as rows of their own.
pub fn extract_grid(
    region: Rect,
    lines: &[Line],
    hrules: &[(Rect, f32)],
    vrules: &[Rect],
    cfg: &TablesConfig,
    lines_cfg: &LinesConfig,
) -> Option<Grid> {
    grid_from_lines(region, lines, hrules, vrules, true, cfg, lines_cfg)
        .or_else(|| grid_from_lines(region, lines, hrules, vrules, false, cfg, lines_cfg))
}

/// One attempt of [`extract_grid`]; `merge_lines`: look for cells wrapped
/// over several lines.
fn grid_from_lines(
    region: Rect,
    lines: &[Line],
    hrules: &[(Rect, f32)],
    vrules: &[Rect],
    merge_lines: bool,
    cfg: &TablesConfig,
    lines_cfg: &LinesConfig,
) -> Option<Grid> {
    let text = table_text(lines, hrules, merge_lines, cfg)?;
    let size = text.size;
    let separators = find_columns(&text, cfg)?;
    let n_cols = separators.len() + 1;
    let mut grid_rows = build_cells(&text.rows, text.multiline, &separators, cfg, lines_cfg);
    let mut baselines: Vec<f32> = text.rows.iter().map(|row| row_baseline(row, text.multiline)).collect();
    let mut last_baselines: Vec<f32> = text.rows.iter().map(|row| last_baseline(row)).collect();
    let floating = if text.multiline {
        Vec::new()
    } else {
        take_floating_labels(&mut grid_rows, &mut baselines, size, cfg)
    };
    if !text.multiline {
        last_baselines = baselines.clone();
    }
    let n_rows = grid_rows.len();
    if n_rows < cfg.min_rows_count {
        return None;
    }

    let mut col_bounds = column_bounds(region, hrules, &separators, &text, cfg);
    let mut row_bounds = row_bounds(&baselines, &last_baselines, size, cfg);
    let horizontal = horizontal_rules(hrules, &col_bounds, &baselines, size, cfg);
    snap_row_bounds(
        &mut row_bounds,
        &horizontal.snapped,
        &baselines,
        &last_baselines,
        size,
        cfg,
    );
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
/// groups them into rows; with `merge_lines`, the lines of cells wrapped
/// over several lines are joined into logical rows (see
/// [`merge_continuation_rows`]). `hrules` are the rules of the region.
/// `None` when there are too few pieces or rows, or when a cell holds
/// two-dimensional maths.
fn table_text<'line>(
    lines: &'line [Line],
    hrules: &[(Rect, f32)],
    merge_lines: bool,
    cfg: &TablesConfig,
) -> Option<TableText<'line>> {
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
    let merged = if merge_lines {
        merge_continuation_rows(&rows, hrules, (x_min, x_max), size, cfg)
    } else {
        None
    };
    let (rows, multiline) = match merged {
        Some(merged) => (merged, true),
        None => (rows, false),
    };
    Some(TableText {
        pieces,
        rows,
        multiline,
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

/// Joins the lines of multi-line cells into logical rows.
///
/// The lines (`rows`, one per baseline) are cut into logical rows where a
/// horizontal rule runs between two of them (a rule at least
/// [`TablesConfig::band_rule_min_ratio`] of the text width long), or, when
/// there are no such rules, where the gap between baselines is clearly
/// wider than the table's tightest line pitch. `text_extent` is the left and
/// right edge of the table's text. Returns the logical rows, each with the
/// pieces of all its lines, or `None` when the table does not have
/// multi-line cells: fewer than [`TablesConfig::band_min_count`] logical
/// rows, too few with several lines, or one that is too tall. A table with
/// evenly spaced single-line rows is never merged.
fn merge_continuation_rows<'line>(
    rows: &[Vec<&'line Line>],
    hrules: &[(Rect, f32)],
    text_extent: (f32, f32),
    size: f32,
    cfg: &TablesConfig,
) -> Option<Vec<Vec<&'line Line>>> {
    if rows.len() < 2 {
        return None;
    }
    let baselines: Vec<f32> = rows.iter().map(|row| row_baseline(row, false)).collect();
    let min_rule = cfg.band_rule_min_ratio * (text_extent.1 - text_extent.0);
    let rule_ys: Vec<f32> = hrules
        .iter()
        .filter(|(rule, _)| rule.width() >= min_rule)
        .map(|(rule, _)| rule.cy())
        .collect();
    let gaps: Vec<f32> = baselines.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let ruled: Vec<bool> = (0..gaps.len())
        .map(|k| rule_ys.iter().any(|&y| y > baselines[k] && y < baselines[k + 1]))
        .collect();
    // Many rules cut the table into logical rows by themselves; a few (the
    // rules of a header) leave the logical rows to the loose gaps.
    let by_rules = ruled.iter().filter(|&&is_ruled| is_ruled).count() >= cfg.band_rules_min_count;
    let mut inner: Vec<f32> = (0..gaps.len()).filter(|&k| !ruled[k]).map(|k| gaps[k]).collect();
    inner.sort_by(|a, b| a.total_cmp(b));
    let pitch = inner.get(inner.len() / 2).copied().unwrap_or(f32::INFINITY);
    // Where a logical row ends after row `k`.
    let breaks: Vec<bool> = (0..gaps.len())
        .map(|k| {
            if by_rules {
                ruled[k]
            } else {
                gaps[k] > cfg.continuation_gap_ratio * pitch.max(cfg.row_break_em * size)
            }
        })
        .collect();
    let mut bands: Vec<Vec<&Line>> = vec![rows[0].clone()];
    let mut piece_counts: Vec<Vec<usize>> = vec![vec![rows[0].len()]];
    for (k, row) in rows.iter().enumerate().skip(1) {
        if breaks[k - 1] {
            bands.push(row.clone());
            piece_counts.push(vec![row.len()]);
        } else {
            bands.last_mut()?.extend(row.iter().copied());
            piece_counts.last_mut()?.push(row.len());
        }
    }
    let line_counts: Vec<usize> = piece_counts.iter().map(Vec::len).collect();
    let multi = line_counts.iter().filter(|&&count| count > 1).count();
    // Cells wrapped over several lines make lines with different numbers of
    // pieces; grouped rows are all complete.
    let ragged = piece_counts
        .iter()
        .filter(|counts| counts.len() > 1 && counts.iter().any(|&count| count != counts[0]))
        .count();
    let plausible = bands.len() >= cfg.band_min_count
        && multi * 10 >= bands.len() * cfg.multiline_bands_tenths
        && ragged * 2 >= multi
        && line_counts.iter().all(|&count| count <= cfg.band_max_lines_count)
        && !stacked_numbers(&bands)
        && continues_text(&bands, size, cfg);
    plausible.then(|| {
        for band in bands.iter_mut() {
            band.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
        }
        bands
    })
}

/// Whether some logical row holds two numbers one under the other: separate
/// rows of figures set at the pitch of lines of text, not a cell wrapped
/// over several lines.
fn stacked_numbers(bands: &[Vec<&Line>]) -> bool {
    bands.iter().any(|band| {
        let numbers: Vec<&&Line> = band.iter().filter(|line| is_number(line.text().trim())).collect();
        numbers.iter().enumerate().any(|(i, line)| {
            numbers[i + 1..]
                .iter()
                .any(|other| other.bbox.x_overlap(&line.bbox) > 0.0 && (other.baseline - line.baseline).abs() > 0.0)
        })
    })
}

/// Whether the second and later lines of the cells in `bands` read as the
/// continuation of the line above: a line under another one (it overlaps it
/// horizontally and is at most [`TablesConfig::continuation_max_dy_em`] font
/// sizes lower) continues it when it starts with a lowercase letter. At
/// least [`TablesConfig::continuation_lowercase_tenths`] tenths of such
/// lines must do so, and two at least. Rows of a table that are merely
/// grouped (rows of numbers, labels with capitals) do not.
fn continues_text(bands: &[Vec<&Line>], size: f32, cfg: &TablesConfig) -> bool {
    let (mut under, mut lowercase) = (0usize, 0usize);
    for band in bands {
        for line in band {
            let has_line_above = band.iter().any(|other| {
                let dy = line.baseline - other.baseline;
                dy > cfg.row_break_em * size
                    && dy <= cfg.continuation_max_dy_em * size
                    && other.bbox.x_overlap(&line.bbox) > 0.0
            });
            if !has_line_above {
                continue;
            }
            under += 1;
            let starts_lowercase = line
                .glyphs
                .iter()
                .find(|glyph| !glyph.text.trim().is_empty())
                .is_some_and(|glyph| glyph.text.chars().next().is_some_and(char::is_lowercase));
            lowercase += starts_lowercase as usize;
        }
    }
    lowercase >= cfg.continuation_min_lowercase_count && lowercase * 10 >= under * cfg.continuation_lowercase_tenths
}

/// Baseline of a row: the median of its pieces, or, for a logical row of
/// multi-line cells (`multiline`), the baseline of its first line.
fn row_baseline(row: &[&Line], multiline: bool) -> f32 {
    if multiline {
        return row.iter().map(|line| line.baseline).fold(f32::INFINITY, f32::min);
    }
    median(&row.iter().map(|line| line.baseline).collect::<Vec<_>>(), 0.0)
}

/// Baseline of the last line of a row.
fn last_baseline(row: &[&Line]) -> f32 {
    row.iter().map(|line| line.baseline).fold(f32::NEG_INFINITY, f32::max)
}

/// The separators of the corridors through the whole table, left to right.
///
/// Corridors crossed by at most a tolerated number of rows come first; the
/// corridors between the numbers of the rows of numbers add those that
/// headers and labels crossing them hide. A separator that cuts through a word
/// of a row crossing its corridor moves to where all the words are apart, if
/// the corridor has such a place.
fn table_corridors(text: &TableText, min_bins: CorridorBins, cfg: &TablesConfig) -> Vec<f32> {
    let rows = &text.rows;
    let n_all = rows.len();
    let tolerance = if n_all < cfg.strict_corridor_below_rows_count {
        0
    } else {
        ((n_all as f32 * cfg.corridor_crossing_ratio) as u16).max(1)
    };
    let mut corridors = find_corridors(rows, text.x_min, text.x_max, tolerance, min_bins);
    for river in numeric_rivers(rows, text, min_bins, cfg) {
        let known = corridors
            .iter()
            .any(|corridor| corridor.start - BIN_PT <= river.end && river.start <= corridor.end + BIN_PT);
        if !known {
            corridors.push(river);
        }
    }
    for corridor in corridors.iter_mut() {
        if !cuts_word(rows, corridor.at) {
            continue;
        }
        if let Some(clear) = clearest_point(rows, corridor.start, corridor.end, true)
            && !cuts_word(rows, clear)
        {
            corridor.at = clear;
        }
    }
    let mut separators: Vec<f32> = corridors.iter().map(|corridor| corridor.at).collect();
    separators.sort_by(|a, b| a.total_cmp(b));
    separators
}

/// Finds the column separators (x positions, left to right): corridors
/// through the whole table, then corridors inside columns that turn out to
/// be two columns with a few spanning cells. `None` when there is no
/// corridor at all.
fn find_columns(text: &TableText, cfg: &TablesConfig) -> Option<Vec<f32>> {
    let rows = &text.rows;
    let min_bins = CorridorBins {
        run: ((cfg.min_corridor_em * text.size / BIN_PT) as usize).max(1),
        clear: ((cfg.min_clear_corridor_em * text.size / BIN_PT) as usize).max(1),
        numeric_run: ((cfg.min_numeric_corridor_em * text.size / BIN_PT) as usize).max(1),
        numeric_rows: cfg.numeric_corridor_min_rows_count,
        words: false,
    };
    let mut separators = table_corridors(text, min_bins, cfg);
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

/// Whether the position `x` falls inside a word of some row.
fn cuts_word(rows: &[Vec<&Line>], x: f32) -> bool {
    rows.iter()
        .flatten()
        .flat_map(|line| piece_words(line))
        .any(|word| word.x0 < x && x < word.x1)
}

/// The corridors between the numbers of the rows of numbers, found word by
/// word with no row crossing them: a header or a label centred over several
/// columns crosses them in the other rows, but not in these.
fn numeric_rivers(rows: &[Vec<&Line>], text: &TableText, min_bins: CorridorBins, cfg: &TablesConfig) -> Vec<Corridor> {
    let numeric_rows: Vec<Vec<&Line>> = rows
        .iter()
        .filter(|row| {
            let words: Vec<Word> = row.iter().flat_map(|line| piece_words(line)).collect();
            let numeric = words.iter().filter(|word| word.numeric).count();
            numeric >= 2 && numeric * 10 >= words.len() * cfg.numeric_row_min_tenths
        })
        .cloned()
        .collect();
    if numeric_rows.len() < cfg.numeric_rivers_min_rows_count {
        return Vec::new();
    }
    let by_words = CorridorBins {
        words: true,
        ..min_bins
    };
    find_corridors(&numeric_rows, text.x_min, text.x_max, 0, by_words)
        .into_iter()
        .map(|corridor| Corridor {
            at: clearest_point(&numeric_rows, corridor.start, corridor.end, true).unwrap_or(corridor.at),
            ..corridor
        })
        .collect()
}

/// Looks for sub-column separators inside the column between `left` and
/// `right` and appends them to `added`: first those that both halves support
/// by a share of the rows, and, when there are none, those that all the rows
/// with two pieces in the column agree on.
fn split_column(
    rows: &[Vec<&Line>],
    left: f32,
    right: f32,
    min_bins: CorridorBins,
    cfg: &TablesConfig,
    added: &mut Vec<f32>,
) {
    let known = added.len();
    split_column_by_share(rows, left, right, min_bins, cfg, added);
    if added.len() == known {
        split_column_by_multi_rows(rows, left, right, min_bins, cfg, added);
    }
}

/// Looks for sub-column separators inside the column between `left` and
/// `right` that both halves support, with a crossing tolerance relative to
/// the rows that use the column. Columns used by too few rows, or whose rows
/// rarely hold two pieces, are left alone.
fn split_column_by_share(
    rows: &[Vec<&Line>],
    left: f32,
    right: f32,
    min_bins: CorridorBins,
    cfg: &TablesConfig,
    added: &mut Vec<f32>,
) {
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

/// Looks for sub-column separators inside the column between `left` and
/// `right` among the rows that hold two or more pieces in it. A corridor
/// that none of these rows crosses and that at least
/// [`multi_row_split_min_rows_count`](TablesConfig::multi_row_split_min_rows_count)
/// of them have pieces on both sides of is a separator; the other rows are
/// then cells spanning both columns (a label centred over them, say).
fn split_column_by_multi_rows(
    rows: &[Vec<&Line>],
    left: f32,
    right: f32,
    min_bins: CorridorBins,
    cfg: &TablesConfig,
    added: &mut Vec<f32>,
) {
    let multi_rows: Vec<Vec<&Line>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .copied()
                .filter(|line| line.bbox.cx() >= left && line.bbox.cx() <= right)
                .collect::<Vec<_>>()
        })
        .filter(|row| row.len() >= 2)
        .collect();
    if multi_rows.len() < cfg.multi_row_split_min_rows_count {
        return;
    }
    let lo = multi_rows
        .iter()
        .flatten()
        .map(|line| line.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    let hi = multi_rows
        .iter()
        .flatten()
        .map(|line| line.bbox.x1)
        .fold(f32::NEG_INFINITY, f32::max);
    for corridor in find_corridors(&multi_rows, lo, hi, 0, min_bins) {
        let split = clearest_point(rows, corridor.start, corridor.end, false).unwrap_or(corridor.at);
        let straddling = multi_rows
            .iter()
            .filter(|row| row.iter().any(|line| line.bbox.x1 <= split) && row.iter().any(|line| line.bbox.x0 >= split))
            .count();
        let (numeric, with_both) = numeric_sides(&multi_rows, split, split);
        // Labels and words that happen to repeat in two rows are not columns.
        if straddling >= cfg.multi_row_split_min_rows_count && numeric >= 1 && numeric * 2 >= with_both {
            added.push(split);
        }
    }
}

/// The middle of the longest stretch of `[start, end]` that the fewest of
/// all `rows` have text in (numbers counted one by one); `None` when the
/// range is empty. Rows that do not take part in finding a corridor can still
/// crowd one side of it.
fn clearest_point(rows: &[Vec<&Line>], start: f32, end: f32, by_words: bool) -> Option<f32> {
    let n_bins = ((end - start) / BIN_PT).ceil() as usize + 1;
    let mut crossing = vec![0u16; n_bins];
    for row in rows {
        let mut covered = vec![false; n_bins];
        for (left, right) in row.iter().flat_map(|line| piece_extents(line, by_words)) {
            if right < start || left > end {
                continue;
            }
            let first_bin = ((left - start) / BIN_PT).floor().max(0.0) as usize;
            let last_bin = (((right - start) / BIN_PT).ceil().max(0.0) as usize).min(n_bins - 1);
            for flag in &mut covered[first_bin..=last_bin.max(first_bin)] {
                *flag = true;
            }
        }
        for (bin, flag) in covered.iter().enumerate() {
            crossing[bin] += *flag as u16;
        }
    }
    let (best, best_len) = emptiest_stretch(&crossing, 0, n_bins);
    (best_len > 0).then_some(start + (best as f32 + best_len as f32 * 0.5) * BIN_PT)
}

/// The separators (x positions) of the corridors found by
/// [`find_corridors`].
fn find_separators(rows: &[Vec<&Line>], x0: f32, x1: f32, tolerance: u16, min_bins: CorridorBins) -> Vec<f32> {
    find_corridors(rows, x0, x1, tolerance, min_bins)
        .into_iter()
        .map(|corridor| corridor.at)
        .collect()
}

/// Finds the vertical corridors within `[x0, x1]`: stretches crossed by at
/// most `tolerance` rows and bounded by text on both sides. Returns the
/// extent of each and the x position of its separator.
///
/// Only pieces whose centre lies inside the range count. A corridor must be
/// at least `min_bins.run` bins (of [`BIN_PT`]) wide; the separator is put in
/// its emptiest stretch, which must be at least `min_bins.clear` bins wide.
fn find_corridors(rows: &[Vec<&Line>], x0: f32, x1: f32, tolerance: u16, min_bins: CorridorBins) -> Vec<Corridor> {
    let crossing = crossing_counts(rows, x0, x1, min_bins.words);
    let n_bins = crossing.len();
    let mut corridors = Vec::new();
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
        if start == 0 || i >= n_bins || i - start < min_bins.numeric_run {
            continue;
        }
        let narrow = i - start < min_bins.run;
        if narrow && !numbers_on_both_sides(rows, x0 + start as f32 * BIN_PT, x0 + i as f32 * BIN_PT, min_bins) {
            continue;
        }
        let (best, best_len) = emptiest_stretch(&crossing, start, i);
        if best_len >= min_bins.clear {
            corridors.push(Corridor {
                start: x0 + start as f32 * BIN_PT,
                end: x0 + i as f32 * BIN_PT,
                at: x0 + (best as f32 + best_len as f32 * 0.5) * BIN_PT,
            });
        }
    }
    corridors
}

/// For each bin of `[x0, x1]`, the number of rows with text in it. A piece
/// counts as one stretch of text, except that the space between two numbers
/// is not covered: two numbers set tightly side by side can be one piece.
fn crossing_counts(rows: &[Vec<&Line>], x0: f32, x1: f32, by_words: bool) -> Vec<u16> {
    let n_bins = ((x1 - x0) / BIN_PT).ceil() as usize + 1;
    let mut crossing = vec![0u16; n_bins];
    for row in rows {
        let mut covered = vec![false; n_bins];
        for line in row {
            // Only pieces that lie inside the range count.
            if line.bbox.cx() < x0 || line.bbox.cx() > x1 {
                continue;
            }
            for (left, right) in piece_extents(line, by_words) {
                let first_bin = ((left - x0) / BIN_PT).floor().max(0.0) as usize;
                let last_bin = (((right - x0) / BIN_PT).ceil().max(0.0) as usize).min(n_bins - 1);
                if first_bin <= last_bin {
                    for flag in &mut covered[first_bin..=last_bin] {
                        *flag = true;
                    }
                }
            }
        }
        for (i, flag) in covered.iter().enumerate() {
            crossing[i] += *flag as u16;
        }
    }
    crossing
}

/// The `(left, right)` extents of the pieces of text to count: the stretches
/// of the piece, or, when `by_words`, each of its words.
fn piece_extents(line: &Line, by_words: bool) -> Vec<(f32, f32)> {
    if by_words {
        piece_words(line).iter().map(|word| (word.x0, word.x1)).collect()
    } else {
        piece_stretches(line)
    }
}

/// The `(left, right)` extents of the stretches of text of a piece: the
/// whole piece, cut between every two neighbouring numbers.
fn piece_stretches(line: &Line) -> Vec<(f32, f32)> {
    let mut stretches: Vec<(f32, f32)> = Vec::new();
    let mut previous_numeric = false;
    for word in piece_words(line) {
        match stretches.last_mut() {
            Some(stretch) if !(previous_numeric && word.numeric) => stretch.1 = word.x1,
            _ => stretches.push((word.x0, word.x1)),
        }
        previous_numeric = word.numeric;
    }
    stretches
}

/// The words of a piece of text, left to right: runs of glyphs with no word
/// space between them.
fn piece_words(line: &Line) -> Vec<Word> {
    let mut words: Vec<(f32, f32, String)> = Vec::new();
    let mut open = false;
    for glyph in &line.glyphs {
        if glyph.text.trim().is_empty() {
            open = false;
            continue;
        }
        match words.last_mut() {
            Some(word) if open && !glyph.space_before => {
                word.0 = word.0.min(glyph.x0);
                word.1 = word.1.max(glyph.x1);
                word.2.push_str(&glyph.text);
            }
            _ => words.push((glyph.x0, glyph.x1, glyph.text.clone())),
        }
        open = true;
    }
    words
        .into_iter()
        .map(|(x0, x1, text)| Word {
            x0,
            x1,
            numeric: is_number(&text),
        })
        .collect()
}

/// Whether a word is a number: it starts with a digit or a sign, has a digit
/// and nothing but digits, signs, separators, a percent sign, an exponent
/// mark or a footnote symbol. A tolerance ("±0.4") belongs to the number
/// before it and is not one itself.
fn is_number(word: &str) -> bool {
    let digits = word.trim_end_matches(|ch| NUMBER_UNITS.contains(ch));
    digits
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit() || "+-−~<>≈.".contains(ch))
        && digits.chars().any(|ch| ch.is_ascii_digit())
        && digits
            .chars()
            .all(|ch| ch.is_ascii_digit() || ".,%±+-−~*†‡<>≈eE".contains(ch))
}

/// Whether the stretch `[xa, xb]` lies between numbers: in enough rows, and
/// in most of the rows that have a word on both sides of it, the nearest
/// words to its left and right are numbers.
fn numbers_on_both_sides(rows: &[Vec<&Line>], xa: f32, xb: f32, min_bins: CorridorBins) -> bool {
    let (numeric, with_both) = numeric_sides(rows, xa, xb);
    numeric >= min_bins.numeric_rows && numeric * 2 > with_both
}

/// For the stretch `[xa, xb]`: how many rows have numbers as the nearest
/// words on both sides of it, and how many have a word on both sides.
fn numeric_sides(rows: &[Vec<&Line>], xa: f32, xb: f32) -> (usize, usize) {
    let (mut numeric, mut with_both) = (0, 0);
    for row in rows {
        let words: Vec<Word> = row.iter().flat_map(|line| piece_words(line)).collect();
        let left = words
            .iter()
            .filter(|word| word.x1 <= xa + BIN_PT)
            .max_by(|a, b| a.x1.total_cmp(&b.x1));
        let right = words
            .iter()
            .filter(|word| word.x0 >= xb - BIN_PT)
            .min_by(|a, b| a.x0.total_cmp(&b.x0));
        if let (Some(left), Some(right)) = (left, right) {
            with_both += 1;
            numeric += (left.numeric && right.numeric) as usize;
        }
    }
    (numeric, with_both)
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
/// its cell span columns, unless it is a row of single-word cells set close
/// together (see [`split_tight_cells`]).
fn build_cells(
    rows: &[Vec<&Line>],
    multiline: bool,
    separators: &[f32],
    cfg: &TablesConfig,
    lines_cfg: &LinesConfig,
) -> Vec<Vec<GridCell>> {
    let col_of = |x: f32| separators.iter().filter(|&&separator| separator <= x).count();
    let words: Vec<Vec<Word>> = rows
        .iter()
        .map(|row| row.iter().flat_map(|line| piece_words(line)).collect())
        .collect();
    let mut grid_rows: Vec<Vec<GridCell>> = Vec::with_capacity(rows.len());
    for (row_idx, row) in rows.iter().enumerate() {
        let others: Vec<&Word> = words
            .iter()
            .enumerate()
            .filter(|(other_idx, _)| *other_idx != row_idx)
            .flat_map(|(_, other)| other.iter())
            .collect();
        let pieces: Vec<RowPiece> = row
            .iter()
            .flat_map(|line| split_tight_cells(line, separators, &others, cfg, lines_cfg))
            .collect();
        let mut cells: Vec<GridCell> = Vec::new();
        let mut placed: Vec<(usize, usize, &Line)> = pieces
            .iter()
            .map(|RowPiece { line, centred }| {
                if *centred {
                    let col = col_of(line.bbox.cx());
                    (col, col, line)
                } else {
                    let first_col = col_of(line.bbox.x0 + cfg.cell_edge_inset_pt);
                    (
                        first_col,
                        col_of(line.bbox.x1 - cfg.cell_edge_inset_pt).max(first_col),
                        line,
                    )
                }
            })
            .collect();
        if multiline {
            // The lines of one cell stand one under the other.
            placed.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.baseline.total_cmp(&b.2.baseline)));
        }
        for (first_col, last_col, line) in placed {
            match cells.last_mut() {
                // Another piece of the same cell.
                Some(prev) if first_col < prev.col + prev.colspan => {
                    prev.colspan = prev.colspan.max(last_col + 1 - prev.col);
                    prev.lines.push(line.clone());
                }
                _ => cells.push(GridCell {
                    col: first_col,
                    colspan: last_col - first_col + 1,
                    rowspan: 1,
                    lines: vec![line.clone()],
                }),
            }
        }
        grid_rows.push(cells);
    }
    grid_rows
}

/// Splits a piece that separators run through into its words when they are
/// cells of their own: the piece has one more word than it crosses
/// separators, each separator lies in the space between two words, and each
/// word is lined up with a word of another row (or there are many words), as the cells of a column are
/// (two numbers set too close together to be told apart, or the labels of a
/// row of columns). A label centred over several columns is not lined up with
/// anything and is left whole, unless it has so many words that each is a
/// label of its own column. `others` are the words of the other rows.
fn split_tight_cells(
    line: &Line,
    separators: &[f32],
    others: &[&Word],
    cfg: &TablesConfig,
    lines_cfg: &LinesConfig,
) -> Vec<RowPiece> {
    let crossed: Vec<f32> = separators
        .iter()
        .copied()
        .filter(|&separator| {
            separator > line.bbox.x0 + cfg.cell_edge_inset_pt && separator < line.bbox.x1 - cfg.cell_edge_inset_pt
        })
        .collect();
    let words = piece_words(line);
    let whole = || {
        vec![RowPiece {
            line: line.clone(),
            centred: false,
        }]
    };
    let pieces = |cuts: &[f32], centred: bool| {
        split_at_positions(line, cuts, lines_cfg)
            .into_iter()
            .map(|line| RowPiece { line, centred })
            .collect()
    };
    if crossed.is_empty() || words.len() != crossed.len() + 1 {
        return whole();
    }
    let in_spaces = crossed
        .iter()
        .enumerate()
        .all(|(i, &separator)| words[i].x1 <= separator && separator <= words[i + 1].x0);
    let own_column = |(i, word): (usize, &Word)| {
        let centre = (word.x0 + word.x1) * 0.5;
        (i == 0 || crossed[i - 1] < centre) && (i == crossed.len() || centre < crossed[i])
    };
    let labels = words.len() >= cfg.split_unaligned_min_words_count && words.iter().enumerate().all(own_column);
    let lined_up = |word: &Word| {
        let tolerance = cfg.aligned_word_tolerance_pt;
        let aligned = others
            .iter()
            .filter(|other| (other.x0 - word.x0).abs() <= tolerance || (other.x1 - word.x1).abs() <= tolerance)
            .count();
        aligned >= cfg.aligned_words_min_count
    };
    if labels && !in_spaces {
        // Centred over its column, each word is cut off in the middle of the
        // space that follows it.
        let cuts: Vec<f32> = words.windows(2).map(|pair| (pair[0].x1 + pair[1].x0) * 0.5).collect();
        pieces(&cuts, true)
    } else if in_spaces && (labels || words.iter().all(lined_up)) {
        pieces(&crossed, false)
    } else {
        whole()
    }
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

/// The `n_rows + 1` row boundaries derived from the baselines (`baselines`:
/// of the first line of each row, `last_baselines`: of its last line, the
/// same for rows of one line). Boundary `k` lies above row `k`; boundary
/// `n_rows` lies below the last row.
fn row_bounds(baselines: &[f32], last_baselines: &[f32], size: f32, cfg: &TablesConfig) -> Vec<f32> {
    let n_rows = baselines.len();
    let mut row_bounds = Vec::with_capacity(n_rows + 1);
    row_bounds.push(baselines[0] - cfg.first_row_top_em * size);
    for k in 1..n_rows {
        row_bounds.push((last_baselines[k - 1] + baselines[k]) * 0.5 - cfg.row_boundary_raise_em * size);
    }
    row_bounds.push(last_baselines[n_rows - 1] + cfg.last_row_bottom_em * size);
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
fn snap_row_bounds(
    row_bounds: &mut [f32],
    snapped: &[Option<f32>],
    baselines: &[f32],
    last_baselines: &[f32],
    size: f32,
    cfg: &TablesConfig,
) {
    let n_rows = baselines.len();
    for k in 0..=n_rows {
        if let Some(y) = snapped[k] {
            let lo = if k == 0 {
                f32::NEG_INFINITY
            } else {
                last_baselines[k - 1] + cfg.snap_min_below_em * size
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
        // A row-spanning cell from an earlier row that covers `col` in `row`.
        let spanning = |rows: &[Vec<GridCell>], row: usize| {
            (0..row).rev().find_map(|start| {
                rows[start]
                    .iter()
                    .position(|other| {
                        other.col <= col && col < other.col + other.colspan && start + other.rowspan > row
                    })
                    .map(|index| (start, index))
            })
        };
        let free = |row: usize| {
            !grid_rows[row]
                .iter()
                .any(|other| other.col <= col && col < other.col + other.colspan)
                && spanning(grid_rows, row).is_none()
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
            // A second line of a label already spanning these rows
            // ("RoBERTa base" / "LoRA"): part of that label.
            if let Some((start, index)) = spanning(grid_rows, near) {
                grid_rows[start][index].lines.extend(cell.lines);
                continue;
            }
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
        && !has_merged_figures(grid_rows, cfg)
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

/// Rule: no column holds two figures side by side in its cells. Several
/// cells made of two or more numeric pieces on one line mean that two
/// columns of numbers were taken for one, which would be a wrong table; the
/// caller shows a picture instead.
fn has_merged_figures(grid_rows: &[Vec<GridCell>], cfg: &TablesConfig) -> bool {
    let merged = grid_rows
        .iter()
        .flatten()
        .filter(|cell| {
            cell.lines.len() >= 2
                && cell.lines.windows(2).all(|pair| {
                    (pair[1].baseline - pair[0].baseline).abs() < cfg.row_break_em * pair[0].size
                        && pair.iter().all(|line| ends_like_figure(&line.text()))
                })
        })
        .count();
    merged >= cfg.merged_figure_cells_min_count
}

/// Whether `text` is a figure with its error bars: it starts with a digit or
/// a sign and ends with a digit.
fn ends_like_figure(text: &str) -> bool {
    let text = text.trim();
    text.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit() || "+-−±~<>≈".contains(ch))
        && text.chars().last().is_some_and(|ch| ch.is_ascii_digit())
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
