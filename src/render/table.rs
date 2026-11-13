//! Reconstructed tables as HTML `<table>`.

use super::*;
use crate::model::*;

/// Cells with at most this many characters get the `nw` (no wrap) class:
/// short entries such as numbers read badly when broken over two lines.
const NOWRAP_MAX_CHARS: usize = 28;

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
            Some(cell) => out.push_str(&cell_html(table, row_idx, cell, tag)),
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
/// its alignment, wrapping and rule classes.
fn cell_html(table: &Table, row_idx: usize, cell: &TableCell, tag: &str) -> String {
    let align = if cell.colspan > 1 {
        "c"
    } else {
        &table.col_align[cell.col.min(table.n_cols - 1)][..1]
    };
    let short = plain(&cell.spans).chars().count() <= NOWRAP_MAX_CHARS;
    let attrs = span_attrs(cell);
    format!(
        "<{tag}{attrs} class=\"{align}{}{}{}\">{}</{tag}>",
        if short { " nw" } else { "" },
        if cell.rowspan > 1 { " mid" } else { "" },
        table_rule_classes(table, row_idx, cell.col, cell.colspan, cell.rowspan),
        spans_html(&cell.spans)
    )
}
