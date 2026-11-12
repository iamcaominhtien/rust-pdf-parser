//! Building the text of blocks: styled spans, de-hyphenation, equation runs and table cells.

use crate::analysis::tables::Grid;
use crate::backend::FontInfo;
use crate::model::Line;
use crate::model::*;
use std::collections::HashSet;

pub(crate) fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(48).collect()
}

// Text assembly

pub(crate) fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphabetic() || c == '-'))
        .filter(|t| !t.is_empty())
}

pub(crate) fn build_vocab<'a>(blocks: impl Iterator<Item = &'a [Line]>) -> HashSet<String> {
    let mut vocab = HashSet::new();
    for lines in blocks {
        let mut prev_hyphen = false;
        for l in lines {
            let text = l.text();
            let ends_hyphen = text.ends_with('-');
            let toks: Vec<&str> = tokens(&text).collect();
            let n = toks.len();
            for (i, t) in toks.into_iter().enumerate() {
                if (i == 0 && prev_hyphen) || (i + 1 == n && ends_hyphen) {
                    continue;
                }
                let t = t.trim_matches('-').to_lowercase();
                if t.is_empty() {
                    continue;
                }
                for part in t.split('-') {
                    if !part.is_empty() {
                        vocab.insert(part.to_string());
                    }
                }
                vocab.insert(t);
            }
            prev_hyphen = ends_hyphen;
        }
    }
    vocab
}

/// Decides whether a line-end hyphen is a soft hyphen (to be removed).
pub(crate) fn remove_hyphen(left: &str, right: &str, vocab: &HashSet<String>) -> bool {
    let trailing: String = left
        .chars()
        .rev()
        .skip(1)
        .take_while(|c| c.is_alphabetic())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let leading: String = right.chars().take_while(|c| c.is_alphabetic()).collect();
    if trailing.is_empty() || leading.is_empty() {
        return false;
    }
    if !leading.chars().next().unwrap().is_lowercase() {
        return false;
    }
    // "state-of-\nthe-art": the left part is itself a hyphenated compound.
    let before: Vec<char> = left.chars().rev().skip(1 + trailing.chars().count()).take(1).collect();
    if before.first() == Some(&'-') {
        return false;
    }
    let (l, r) = (trailing.to_lowercase(), leading.to_lowercase());
    if vocab.contains(&format!("{l}{r}")) {
        return true;
    }
    if vocab.contains(&format!("{l}-{r}")) {
        return false;
    }
    if l.chars().count() >= 4 && vocab.contains(&l) && vocab.contains(&r) {
        return false;
    }
    true
}

pub(crate) struct TextBuilder<'a> {
    pub(crate) fonts: &'a [FontInfo],
    pub(crate) vocab: &'a HashSet<String>,
}

pub(crate) struct Built {
    /// The running text of the block.
    pub(crate) spans: Vec<Span>,
    pub(crate) line_texts: Vec<String>,
    pub(crate) hlines: Vec<HLine>,
}

impl TextBuilder<'_> {
    /// Styled text of a single line, exactly as it appears.
    pub(crate) fn line_spans(&self, line: &Line) -> Vec<Span> {
        let mut spans: Vec<Span> = Vec::new();
        for (gi, g) in line.glyphs.iter().enumerate() {
            let style = line.style_of(g, self.fonts);
            let space = g.space_before && gi > 0;
            match spans.last_mut() {
                Some(s) if s.style == style && s.color == g.color => {
                    if space {
                        s.text.push(' ');
                    }
                    s.text.push_str(&g.text);
                }
                Some(s) => {
                    if space {
                        s.text.push(' ');
                    }
                    spans.push(Span {
                        text: g.text.clone(),
                        style,
                        href: None,
                        color: g.color,
                    });
                }
                None => spans.push(Span {
                    text: g.text.clone(),
                    style,
                    href: None,
                    color: g.color,
                }),
            }
        }
        spans
    }

    /// Converts lines into styled spans, joining the lines into running text.
    /// `skip` glyphs are dropped from the start of the running text and
    /// `mask` style bits are cleared in it (e.g. bold inside headings). The
    /// per-line output keeps everything.
    pub(crate) fn build(&self, lines: &[Line], skip: usize, mask: u8) -> Built {
        let texts: Vec<String> = lines.iter().map(|l| l.text()).collect();
        let mut hlines: Vec<HLine> = Vec::with_capacity(lines.len());
        let mut line_texts: Vec<String> = Vec::with_capacity(lines.len());
        for (li, line) in lines.iter().enumerate() {
            let mut ls = self.line_spans(line);
            let mut soft_hyphen = false;
            if li + 1 < lines.len() {
                let lt = &texts[li];
                if lt.ends_with('-') && lt.chars().count() > 1 {
                    if remove_hyphen(lt, &texts[li + 1], self.vocab) {
                        if let Some(last) = ls.last_mut() {
                            last.text.pop();
                            if last.text.is_empty() {
                                ls.pop();
                            }
                        }
                        soft_hyphen = true;
                    }
                } else if !lt.ends_with('/')
                    && !lt.is_empty()
                    && let Some(last) = ls.last_mut()
                {
                    last.text.push(' ');
                }
            }
            line_texts.push(ls.iter().map(|s| s.text.as_str()).collect());
            hlines.push(HLine { spans: ls, soft_hyphen });
        }
        let mut spans: Vec<Span> = Vec::new();
        for s in hlines.iter().flat_map(|h| h.spans.iter()) {
            let style = s.style & !mask;
            match spans.last_mut() {
                Some(p) if p.style == style && p.color == s.color => p.text.push_str(&s.text),
                _ => spans.push(Span {
                    text: s.text.clone(),
                    style,
                    href: None,
                    color: s.color,
                }),
            }
        }
        if skip > 0 && !lines.is_empty() {
            let n: usize = lines[0].glyphs.iter().take(skip).map(|g| g.text.chars().count()).sum();
            strip_chars(&mut spans, n);
        }
        Built {
            spans,
            line_texts,
            hlines,
        }
    }

    /// Positioned glyph runs of a displayed formula.
    pub(crate) fn eq_runs(&self, lines: &[Line]) -> Vec<EqRun> {
        let mut runs: Vec<EqRun> = Vec::new();
        for line in lines {
            // (run, font, right edge)
            let mut cur: Option<(EqRun, u16, f32)> = None;
            for g in &line.glyphs {
                // No Unicode value: the glyph stays part of the page picture.
                if g.text.contains('\u{FFFD}') {
                    if let Some((r, _, _)) = cur.take() {
                        runs.push(r);
                    }
                    continue;
                }
                let style = line.style_of(g, self.fonts) & !(SUP | SUB);
                let axis = g.oy - 0.25 * g.size;
                let odd =
                    g.ink.height() > 1.3 * g.size || (g.ink.cy() - axis).abs() > 0.6 * g.size || g.text == "\u{221A}";
                if odd {
                    if let Some((r, _, _)) = cur.take() {
                        runs.push(r);
                    }
                    runs.push(EqRun {
                        text: g.text.clone(),
                        x: g.ox,
                        y: g.oy,
                        width: (g.x1 - g.x0).max(0.1),
                        size: g.size,
                        style,
                        color: g.color,
                        ink: Some(g.ink),
                    });
                    continue;
                }
                let joins = cur.as_ref().is_some_and(|(r, font, end)| {
                    *font == g.font
                        && (r.size - g.size).abs() < 0.1
                        && (r.y - g.oy).abs() < 0.2
                        && r.color == g.color
                        && g.x0 - end < 0.6 * g.size
                        && g.x0 >= r.x
                });
                if joins {
                    let (r, _, end) = cur.as_mut().unwrap();
                    if g.space_before {
                        r.text.push(' ');
                    }
                    r.text.push_str(&g.text);
                    *end = g.x1;
                    r.width = g.x1 - r.x;
                } else {
                    if let Some((r, _, _)) = cur.take() {
                        runs.push(r);
                    }
                    cur = Some((
                        EqRun {
                            text: g.text.clone(),
                            x: g.x0,
                            y: g.oy,
                            width: (g.x1 - g.x0).max(0.1),
                            size: g.size,
                            style,
                            color: g.color,
                            ink: None,
                        },
                        g.font,
                        g.x1,
                    ));
                }
            }
            if let Some((r, _, _)) = cur.take() {
                runs.push(r);
            }
        }
        runs
    }

    pub(crate) fn table(&self, grid: Grid) -> Table {
        let rows = grid
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        let mut spans: Vec<Span> = Vec::new();
                        for (i, l) in cell.lines.iter().enumerate() {
                            if i > 0
                                && let Some(last) = spans.last_mut()
                            {
                                last.text.push(' ');
                            }
                            spans.extend(self.line_spans(l));
                        }
                        TableCell {
                            col: cell.col,
                            colspan: cell.colspan,
                            rowspan: cell.rowspan,
                            spans,
                        }
                    })
                    .collect()
            })
            .collect();
        Table {
            n_cols: grid.n_cols,
            header_rows: grid.header_rows,
            col_align: grid
                .col_align
                .iter()
                .map(|a| match a {
                    Align::Left => "left",
                    Align::Center => "center",
                    Align::Right => "right",
                })
                .collect(),
            rows,
            rule_above: grid.rule_above,
            rule_bottom: grid.rule_bottom,
            rule_left: grid.rule_left,
            rule_right: grid.rule_right,
            geom: TableGeom {
                bbox: grid.bbox,
                col_bounds: grid.col_bounds,
                col_text: grid.col_text,
                row_bounds: grid.row_bounds,
                row_baselines: grid.row_baselines,
                size: grid.size,
            },
        }
    }
}

/// Removes the first `n` characters from a span list.
pub(crate) fn strip_chars(spans: &mut Vec<Span>, mut n: usize) {
    while n > 0 && !spans.is_empty() {
        let len = spans[0].text.chars().count();
        if len <= n {
            n -= len;
            spans.remove(0);
        } else {
            spans[0].text = spans[0].text.chars().skip(n).collect();
            n = 0;
        }
    }
    if let Some(s) = spans.first_mut() {
        s.text = s.text.trim_start().to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    fn vocab(words: &[&str]) -> HashSet<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn soft_hyphens_are_removed() {
        let v = vocab(&["multi", "head", "multi-head", "self", "attention", "information"]);
        // Unknown fragments: a plain word break.
        assert!(remove_hyphen("dot-product (multi-", "plicative) attention", &v));
        // The joined word is known.
        assert!(remove_hyphen("more in-", "formation is needed", &v));
        // Both halves are words of their own and the compound is not known.
        assert!(!remove_hyphen("layers of self-", "attention are used", &v));
        // A compound seen elsewhere in the document keeps its hyphen.
        assert!(!remove_hyphen("we use multi-", "head attention", &v));
        // Capitalised or non-alphabetic continuations keep the hyphen.
        assert!(!remove_hyphen("the English-", "German task", &v));
        assert!(!remove_hyphen("pages 10-", "20 of the", &v));
        // Part of a longer hyphenated compound.
        assert!(!remove_hyphen("a state-of-", "the-art model", &v));
    }
}
