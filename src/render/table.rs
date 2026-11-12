//! Reconstructed tables as HTML `<table>`.

use super::*;
use crate::model::*;

/// Classes describing the rules around the cell at (`row`, `col`) spanning
/// `colspan` x `rowspan` positions: `ra1`/`ra2` thin/heavy rule above, `rb*`
/// below, `rl`/`rr` vertical rules.
pub(crate) fn table_rule_classes(t: &Table, row: usize, col: usize, colspan: usize, rowspan: usize) -> String {
    let n_rows = t.rows.len();
    let end = (col + colspan).min(t.n_cols);
    let last_row = (row + rowspan).min(n_rows) - 1;
    let mut cls = String::new();
    let above = t.rule_above[row][col..end].iter().copied().max().unwrap_or(0);
    if above > 0 {
        cls.push_str(&format!(" ra{above}"));
    }
    if last_row + 1 == n_rows {
        let below = t.rule_bottom[col..end].iter().copied().max().unwrap_or(0);
        if below > 0 {
            cls.push_str(&format!(" rb{below}"));
        }
    }
    if t.rule_left[row][col] {
        cls.push_str(" rl");
    }
    if end == t.n_cols && t.rule_right[row] {
        cls.push_str(" rr");
    }
    cls
}

/// Walks the cells of a table in document order, including the empty
/// positions, and calls `emit(row, col, cell)` for each.
pub(crate) fn table_walk(t: &Table, mut emit: impl FnMut(usize, usize, Option<&TableCell>)) {
    let n_rows = t.rows.len();
    // Positions covered by a row-spanning cell from an earlier row.
    let mut covered = vec![vec![false; t.n_cols]; n_rows];
    for (ri, row) in t.rows.iter().enumerate() {
        let mut c = 0;
        for cell in row {
            while c < cell.col.min(t.n_cols) {
                if !covered[ri][c] {
                    emit(ri, c, None);
                }
                c += 1;
            }
            emit(ri, cell.col, Some(cell));
            let rows_end = (ri + cell.rowspan).min(n_rows);
            let cols_end = (cell.col + cell.colspan).min(t.n_cols);
            for row_covered in covered.iter_mut().take(rows_end).skip(ri + 1) {
                for cov in row_covered.iter_mut().take(cols_end).skip(cell.col) {
                    *cov = true;
                }
            }
            c = cell.col + cell.colspan;
        }
        while c < t.n_cols {
            if !covered[ri][c] {
                emit(ri, c, None);
            }
            c += 1;
        }
    }
}

/// A reconstructed table as an HTML `<table>`.
pub(crate) fn table_html(t: &Table) -> String {
    let mut out = String::from("<table class=\"tbl\">");
    let mut open_row: Option<usize> = None;
    table_walk(t, |ri, col, cell| {
        if open_row != Some(ri) {
            if open_row.is_some() {
                out.push_str("</tr>");
            }
            if ri == 0 && t.header_rows > 0 {
                out.push_str("<thead>");
            }
            if ri == t.header_rows {
                if t.header_rows > 0 {
                    out.push_str("</thead>");
                }
                out.push_str("<tbody>");
            }
            out.push_str("<tr>");
            open_row = Some(ri);
        }
        let tag = if ri < t.header_rows { "th" } else { "td" };
        match cell {
            None => out.push_str(&format!(
                "<{tag} class=\"e{}\"></{tag}>",
                table_rule_classes(t, ri, col, 1, 1)
            )),
            Some(cell) => {
                let align = if cell.colspan > 1 {
                    "c"
                } else {
                    &t.col_align[cell.col.min(t.n_cols - 1)][..1]
                };
                let short = plain(&cell.spans).chars().count() <= 28;
                let mut attrs = String::new();
                if cell.colspan > 1 {
                    attrs.push_str(&format!(" colspan=\"{}\"", cell.colspan));
                }
                if cell.rowspan > 1 {
                    attrs.push_str(&format!(" rowspan=\"{}\"", cell.rowspan));
                }
                out.push_str(&format!(
                    "<{tag}{attrs} class=\"{align}{}{}{}\">{}</{tag}>",
                    if short { " nw" } else { "" },
                    if cell.rowspan > 1 { " mid" } else { "" },
                    table_rule_classes(t, ri, cell.col, cell.colspan, cell.rowspan),
                    spans_html(&cell.spans)
                ));
            }
        }
    });
    if open_row.is_some() {
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    out
}
