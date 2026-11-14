//! Reconstructed tables as HTML `<table>`.

use super::*;
use crate::model::*;
use regex::Regex;
use std::sync::LazyLock;

/// Cells with at most this many characters get the `nw` (no wrap) class:
/// short entries such as numbers read badly when broken over two lines.
const NOWRAP_MAX_CHARS: usize = 28;

/// The text of a numeric table cell: digits with decimal and thousands
/// separators, slashes, `±`, `%`, minus signs and dashes, brackets, `×`.
static NUMERIC_CELL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\d.,/±%−\-–()×\s]+$").unwrap());

/// Classes describing the rules around the cell at (`row`, `col`) spanning
/// `colspan` x `rowspan` positions: `ra1`/`ra2` thin/heavy rule above, `rb*`
/// below, `rl`/`rr` vertical rules.
pub(crate) fn table_rule_classes(table: &Table, row: usize, col: usize, colspan: usize, rowspan: usize) -> String {
    let n_rows = table.rows.len();
    let end = (col + colspan).min(table.n_cols);
    let last_row = (row + rowspan).min(n_rows) - 1;
    let mut cls = String::new();
    let above = table.rule_above[row][col..end].iter().copied().max().unwrap_or(0);
    if above > 0 {
        cls.push_str(&format!(" ra{above}"));
    }
    if last_row + 1 == n_rows {
        let below = table.rule_bottom[col..end].iter().copied().max().unwrap_or(0);
        if below > 0 {
            cls.push_str(&format!(" rb{below}"));
        }
    }
    if table.rule_left[row][col] {
        cls.push_str(" rl");
    }
    if end == table.n_cols && table.rule_right[row] {
        cls.push_str(" rr");
    }
    cls
}

/// Walks the cells of a table in document order, including the empty
/// positions, and calls `emit(row, col, cell)` for each. Positions covered
/// by a cell spanning several rows are not reported again.
pub(crate) fn table_walk(table: &Table, mut emit: impl FnMut(usize, usize, Option<&TableCell>)) {
    let n_rows = table.rows.len();
    // Positions covered by a row-spanning cell from an earlier row.
    let mut covered = vec![vec![false; table.n_cols]; n_rows];
    for (row_idx, row) in table.rows.iter().enumerate() {
        let mut col = 0;
        for cell in row {
            while col < cell.col.min(table.n_cols) {
                if !covered[row_idx][col] {
                    emit(row_idx, col, None);
                }
                col += 1;
            }
            emit(row_idx, cell.col, Some(cell));
            let rows_end = (row_idx + cell.rowspan).min(n_rows);
            let cols_end = (cell.col + cell.colspan).min(table.n_cols);
            for row_covered in covered.iter_mut().take(rows_end).skip(row_idx + 1) {
                for cov in row_covered.iter_mut().take(cols_end).skip(cell.col) {
                    *cov = true;
                }
            }
            col = cell.col + cell.colspan;
        }
        while col < table.n_cols {
            if !covered[row_idx][col] {
                emit(row_idx, col, None);
            }
            col += 1;
        }
    }
}

/// A reconstructed table as an HTML `<table>`.
pub(crate) fn table_html(table: &Table) -> String {
    write_table(table, &[])
}

/// A reconstructed table as an HTML `<table>`, with the `num` class on the
/// cells of numeric columns (see [`numeric_columns`]) so they align right.
pub(crate) fn table_html_numeric(table: &Table) -> String {
    write_table(table, &numeric_columns(table))
}

/// For each column, whether all its non-empty body cells hold a number
/// (digits with `.`, `,`, `/`, `±`, `%`, minus signs, brackets, `×`).
/// Header-only and spanning cells do not count.
fn numeric_columns(table: &Table) -> Vec<bool> {
    let mut numeric = vec![true; table.n_cols];
    let mut seen = vec![false; table.n_cols];
    for row in table.rows.iter().skip(table.header_rows) {
        for cell in row.iter().filter(|cell| cell.colspan == 1 && cell.col < table.n_cols) {
            let text = plain(&cell.spans);
            if text.is_empty() {
                continue;
            }
            seen[cell.col] = true;
            if !NUMERIC_CELL.is_match(&text) {
                numeric[cell.col] = false;
            }
        }
    }
    numeric
        .iter()
        .zip(&seen)
        .map(|(numeric, seen)| *numeric && *seen)
        .collect()
}

/// Writes a table; `numeric` tells which columns get the `num` class (may
/// be empty: none).
fn write_table(table: &Table, numeric: &[bool]) -> String {
    let mut out = String::from("<table class=\"tbl\">");
    let mut open_row: Option<usize> = None;
    table_walk(table, |row_idx, col, cell| {
        if open_row != Some(row_idx) {
            open_table_row(table, open_row, row_idx, &mut out);
            open_row = Some(row_idx);
        }
        let tag = if row_idx < table.header_rows { "th" } else { "td" };
        match cell {
            None => out.push_str(&format!(
                "<{tag} class=\"e{}\"></{tag}>",
                table_rule_classes(table, row_idx, col, 1, 1)
            )),
            Some(cell) => {
                let num = cell.colspan == 1 && numeric.get(cell.col).copied().unwrap_or(false);
                out.push_str(&cell_html(table, row_idx, cell, tag, num));
            }
        }
    });
    if open_row.is_some() {
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    out
}

/// The ` colspan`/` rowspan` attributes of a cell that spans more than one
/// position (empty for a simple cell).
pub(crate) fn span_attrs(cell: &TableCell) -> String {
    let mut attrs = String::new();
    if cell.colspan > 1 {
        attrs.push_str(&format!(" colspan=\"{}\"", cell.colspan));
    }
    if cell.rowspan > 1 {
        attrs.push_str(&format!(" rowspan=\"{}\"", cell.rowspan));
    }
    attrs
}

/// Closes the open row (if any) and opens row `row_idx`, with the `<thead>`
/// and `<tbody>` tags where the header starts and ends.
fn open_table_row(table: &Table, open_row: Option<usize>, row_idx: usize, out: &mut String) {
    if open_row.is_some() {
        out.push_str("</tr>");
    }
    if row_idx == 0 && table.header_rows > 0 {
        out.push_str("<thead>");
    }
    if row_idx == table.header_rows {
        if table.header_rows > 0 {
            out.push_str("</thead>");
        }
        out.push_str("<tbody>");
    }
    out.push_str("<tr>");
}

/// A non-empty cell of row `row_idx` as a `tag` (`th` or `td`) element, with
/// its alignment, wrapping and rule classes (and `num` when `num`).
fn cell_html(table: &Table, row_idx: usize, cell: &TableCell, tag: &str, num: bool) -> String {
    let align = if cell.colspan > 1 {
        "c"
    } else {
        &table.col_align[cell.col.min(table.n_cols - 1)][..1]
    };
    let short = plain(&cell.spans).chars().count() <= NOWRAP_MAX_CHARS;
    let attrs = span_attrs(cell);
    format!(
        "<{tag}{attrs} class=\"{align}{}{}{}{}\">{}</{tag}>",
        if num { " num" } else { "" },
        if short { " nw" } else { "" },
        if cell.rowspan > 1 { " mid" } else { "" },
        table_rule_classes(table, row_idx, cell.col, cell.colspan, cell.rowspan),
        spans_html(&cell.spans)
    )
}
