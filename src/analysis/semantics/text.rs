//! Building the text of blocks: styled spans, de-hyphenation, equation runs and table cells.
//!
//! [`TextBuilder`] turns the glyph lines of a block into the styled
//! [`Span`]s of the output model. Line-end hyphens are removed when the
//! document's own vocabulary (see [`build_vocab`]) says they are soft.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use crate::analysis::tables::Grid;
use crate::backend::FontInfo;
use crate::model::Line;
use crate::model::*;
use std::collections::HashSet;

/// Most characters of an anchor slug; longer headings are cut, which keeps
/// the URL fragments readable.
const MAX_SLUG_CHARS: usize = 48;

/// Smallest width (points) given to an equation run, so that zero-width
/// glyphs still produce a positioned box.
const MIN_RUN_WIDTH_PT: f32 = 0.1;

/// Tuning parameters of text building. Part of [`SemanticsConfig`](super::SemanticsConfig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextConfig {
    /// At a line-end hyphen whose halves are both words of the document,
    /// the hyphen is kept when the left half has at least this many
    /// characters ("multi-" + "head"); shorter left halves are usually
    /// prefixes broken off a single word.
    pub compound_part_min_chars_count: usize,
    /// Height of the math axis above the baseline (multiple of the font
    /// size); glyphs centred far from it are big operators or delimiters.
    pub math_axis_height_em: f32,
    /// A formula glyph whose ink is taller than this (multiple of the font
    /// size) is drawn on its own (big operators, tall delimiters).
    pub tall_glyph_min_height_em: f32,
    /// A formula glyph whose ink centre is further than this from the math
    /// axis (multiple of the font size) is drawn on its own.
    pub off_axis_min_offset_em: f32,
    /// Glyphs whose sizes differ by less than this (points) may share an
    /// equation run.
    pub run_size_tolerance_pt: f32,
    /// Glyphs whose baselines differ by less than this (points) may share
    /// an equation run.
    pub run_baseline_tolerance_pt: f32,
    /// A glyph starting less than this (multiple of its font size) after
    /// the end of the current run joins it.
    pub run_max_gap_em: f32,
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            compound_part_min_chars_count: 4,
            math_axis_height_em: 0.25,
            tall_glyph_min_height_em: 1.3,
            off_axis_min_offset_em: 0.6,
            run_size_tolerance_pt: 0.1,
            run_baseline_tolerance_pt: 0.2,
            run_max_gap_em: 0.6,
        }
    }
}

/// Converts lines of glyphs into styled spans.
pub(crate) struct TextBuilder<'a> {
    /// Font table, for the style of each glyph.
    pub(crate) fonts: &'a [FontInfo],
    /// Words of the document (see [`build_vocab`]), for de-hyphenation.
    pub(crate) vocab: &'a HashSet<String>,
    /// Thresholds of text building.
    pub(crate) cfg: &'a TextConfig,
}

/// The text of a block, built by [`TextBuilder::build`].
pub(crate) struct Built {
    /// The running text of the block.
    pub(crate) spans: Vec<Span>,
    /// Plain text of each line, as it appears in the running text.
    pub(crate) line_texts: Vec<String>,
    /// Styled spans of each line, for the position-preserving renderers.
    pub(crate) hlines: Vec<HLine>,
}

/// An equation run that may still grow.
struct OpenRun {
    /// The run so far.
    run: EqRun,
    /// Font of its glyphs.
    font: u16,
    /// Right edge of its last glyph.
    end: f32,
}

impl TextBuilder<'_> {
    /// Styled text of a single line, exactly as it appears.
    pub(crate) fn line_spans(&self, line: &Line) -> Vec<Span> {
        let mut spans: Vec<Span> = Vec::new();
        for (glyph_idx, glyph) in line.glyphs.iter().enumerate() {
            let style = line.style_of(glyph, self.fonts);
            let space = glyph.space_before && glyph_idx > 0;
            if let Some(span) = spans.last_mut() {
                if space {
                    span.text.push(' ');
                }
                if span.style == style && span.color == glyph.color {
                    span.text.push_str(&glyph.text);
                    continue;
                }
            }
            spans.push(Span {
                text: glyph.text.clone(),
                style,
                href: None,
                color: glyph.color,
            });
        }
        spans
    }

    /// Converts lines into styled spans, joining the lines into running text.
    /// `skip` glyphs are dropped from the start of the running text and
    /// `mask` style bits are cleared in it (e.g. bold inside headings). The
    /// per-line output keeps everything.
    pub(crate) fn build(&self, lines: &[Line], skip: usize, mask: u8) -> Built {
        let texts: Vec<String> = lines.iter().map(|line| line.text()).collect();
        let mut hlines: Vec<HLine> = Vec::with_capacity(lines.len());
        let mut line_texts: Vec<String> = Vec::with_capacity(lines.len());
        for (line_idx, line) in lines.iter().enumerate() {
            let mut line_spans = self.line_spans(line);
            let soft_hyphen = match texts.get(line_idx + 1) {
                Some(next) => self.join_line_end(&mut line_spans, &texts[line_idx], next),
                None => false,
            };
            line_texts.push(line_spans.iter().map(|span| span.text.as_str()).collect());
            hlines.push(HLine {
                spans: line_spans,
                soft_hyphen,
            });
        }
        let mut spans = running_spans(&hlines, mask);
        if skip > 0 && !lines.is_empty() {
            let n: usize = lines[0]
                .glyphs
                .iter()
                .take(skip)
                .map(|glyph| glyph.text.chars().count())
                .sum();
            strip_chars(&mut spans, n);
        }
        Built {
            spans,
            line_texts,
            hlines,
        }
    }

    /// Prepares the spans of a line (text `text`) to be followed by the next
    /// line (text `next`): removes a soft hyphen, or adds the space between
    /// the lines (none after a slash, as in URLs). Returns whether a soft
    /// hyphen was removed.
    fn join_line_end(&self, spans: &mut Vec<Span>, text: &str, next: &str) -> bool {
        if text.ends_with('-') && text.chars().count() > 1 {
            if remove_hyphen(text, next, self.vocab, self.cfg) {
                if let Some(last) = spans.last_mut() {
                    last.text.pop();
                    if last.text.is_empty() {
                        spans.pop();
                    }
                }
                return true;
            }
        } else if !text.ends_with('/')
            && !text.is_empty()
            && let Some(last) = spans.last_mut()
        {
            last.text.push(' ');
        }
        false
    }

    /// Positioned glyph runs of a displayed formula. Neighbouring glyphs of
    /// the same font, size, baseline and colour form one run; tall or
    /// off-axis glyphs (big operators, radicals) get a run of their own with
    /// their ink box; glyphs without a Unicode value are left out (they stay
    /// part of the page picture).
    pub(crate) fn eq_runs(&self, lines: &[Line]) -> Vec<EqRun> {
        let mut runs: Vec<EqRun> = Vec::new();
        for line in lines {
            let mut current: Option<OpenRun> = None;
            for glyph in &line.glyphs {
                // No Unicode value: the glyph stays part of the page picture.
                if glyph.text.contains('\u{FFFD}') {
                    close_run(&mut current, &mut runs);
                    continue;
                }
                let style = line.style_of(glyph, self.fonts) & !(SUP | SUB);
                if self.stands_alone(glyph) {
                    close_run(&mut current, &mut runs);
                    runs.push(EqRun {
                        text: glyph.text.clone(),
                        x: glyph.ox,
                        y: glyph.oy,
                        width: (glyph.x1 - glyph.x0).max(MIN_RUN_WIDTH_PT),
                        size: glyph.size,
                        style,
                        color: glyph.color,
                        ink: Some(glyph.ink),
                    });
                    continue;
                }
                match current.as_mut().filter(|open| self.extends_run(open, glyph)) {
                    Some(open) => {
                        if glyph.space_before {
                            open.run.text.push(' ');
                        }
                        open.run.text.push_str(&glyph.text);
                        open.end = glyph.x1;
                        open.run.width = glyph.x1 - open.run.x;
                    }
                    None => {
                        close_run(&mut current, &mut runs);
                        current = Some(OpenRun {
                            run: EqRun {
                                text: glyph.text.clone(),
                                x: glyph.x0,
                                y: glyph.oy,
                                width: (glyph.x1 - glyph.x0).max(MIN_RUN_WIDTH_PT),
                                size: glyph.size,
                                style,
                                color: glyph.color,
                                ink: None,
                            },
                            font: glyph.font,
                            end: glyph.x1,
                        });
                    }
                }
            }
            close_run(&mut current, &mut runs);
        }
        runs
    }

    /// Whether a formula glyph is drawn on its own: much taller than the
    /// font size, centred far from the math axis, or a radical sign.
    fn stands_alone(&self, glyph: &LineGlyph) -> bool {
        let axis = glyph.oy - self.cfg.math_axis_height_em * glyph.size;
        glyph.ink.height() > self.cfg.tall_glyph_min_height_em * glyph.size
            || (glyph.ink.cy() - axis).abs() > self.cfg.off_axis_min_offset_em * glyph.size
            || glyph.text == "\u{221A}"
    }

    /// Whether `glyph` continues the open run: same font, size, baseline and
    /// colour, close after its end and not left of its start.
    fn extends_run(&self, open: &OpenRun, glyph: &LineGlyph) -> bool {
        open.font == glyph.font
            && (open.run.size - glyph.size).abs() < self.cfg.run_size_tolerance_pt
            && (open.run.y - glyph.oy).abs() < self.cfg.run_baseline_tolerance_pt
            && open.run.color == glyph.color
            && glyph.x0 - open.end < self.cfg.run_max_gap_em * glyph.size
            && glyph.x0 >= open.run.x
    }

    /// Converts a table grid into the output table, with the styled text of
    /// every cell (the lines of a cell joined by spaces).
    pub(crate) fn table(&self, grid: Grid) -> Table {
        let rows = grid
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| TableCell {
                        col: cell.col,
                        colspan: cell.colspan,
                        rowspan: cell.rowspan,
                        spans: self.cell_spans(&cell.lines),
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
                .map(|align| match align {
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

    /// Styled text of a table cell: its lines joined by single spaces.
    fn cell_spans(&self, lines: &[Line]) -> Vec<Span> {
        let mut spans: Vec<Span> = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            if line_idx > 0
                && let Some(last) = spans.last_mut()
            {
                last.text.push(' ');
            }
            spans.extend(self.line_spans(line));
        }
        spans
    }
}

/// Anchor slug of a text: lowercase letters and digits, every other run of
/// characters replaced by one hyphen, at most [`MAX_SLUG_CHARS`] characters.
pub(crate) fn slug(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(MAX_SLUG_CHARS).collect()
}

/// The words of a text: maximal runs of letters and hyphens.
pub(crate) fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|ch: char| !(ch.is_alphabetic() || ch == '-'))
        .filter(|token| !token.is_empty())
}

/// The vocabulary of the document: every lowercase word (and every part of
/// a hyphenated word) that does not touch a line-end hyphen. Used to decide
/// whether a hyphen at a line break is part of the word.
pub(crate) fn build_vocab<'a>(blocks: impl Iterator<Item = &'a [Line]>) -> HashSet<String> {
    let mut vocab = HashSet::new();
    for lines in blocks {
        let mut prev_hyphen = false;
        for line in lines {
            let text = line.text();
            let ends_hyphen = text.ends_with('-');
            let line_tokens: Vec<&str> = tokens(&text).collect();
            let n = line_tokens.len();
            for (i, token) in line_tokens.into_iter().enumerate() {
                if (i == 0 && prev_hyphen) || (i + 1 == n && ends_hyphen) {
                    continue;
                }
                let word = token.trim_matches('-').to_lowercase();
                if word.is_empty() {
                    continue;
                }
                for part in word.split('-') {
                    if !part.is_empty() {
                        vocab.insert(part.to_string());
                    }
                }
                vocab.insert(word);
            }
            prev_hyphen = ends_hyphen;
        }
    }
    vocab
}

/// Decides whether a line-end hyphen is a soft hyphen (to be removed).
///
/// `left` is the text of the line ending with the hyphen, `right` the text
/// of the next line. The hyphen is kept when the continuation does not
/// start with a lowercase letter, when the left part is itself a
/// hyphenated compound ("state-of-"), when the hyphenated form is known
/// from the document, or when both halves are known words and the left one
/// is long enough; otherwise (including when the joined word is known, or
/// nothing is known) it is removed.
pub(crate) fn remove_hyphen(left: &str, right: &str, vocab: &HashSet<String>, cfg: &TextConfig) -> bool {
    let trailing: String = left
        .chars()
        .rev()
        .skip(1)
        .take_while(|ch| ch.is_alphabetic())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let leading: String = right.chars().take_while(|ch| ch.is_alphabetic()).collect();
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
    let (left_part, right_part) = (trailing.to_lowercase(), leading.to_lowercase());
    if vocab.contains(&format!("{left_part}{right_part}")) {
        return true;
    }
    if vocab.contains(&format!("{left_part}-{right_part}")) {
        return false;
    }
    if left_part.chars().count() >= cfg.compound_part_min_chars_count
        && vocab.contains(&left_part)
        && vocab.contains(&right_part)
    {
        return false;
    }
    true
}

/// Removes the first `n` characters from a span list (dropping spans that
/// become empty), then the whitespace at the start of the first span.
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
    if let Some(first) = spans.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
}

/// Joins the line spans of a block into the running text: neighbouring
/// spans of the same style (after clearing the `mask` bits) and colour are
/// merged.
fn running_spans(hlines: &[HLine], mask: u8) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for span in hlines.iter().flat_map(|hline| hline.spans.iter()) {
        let style = span.style & !mask;
        match spans.last_mut() {
            Some(prev) if prev.style == style && prev.color == span.color => prev.text.push_str(&span.text),
            _ => spans.push(Span {
                text: span.text.clone(),
                style,
                href: None,
                color: span.color,
            }),
        }
    }
    spans
}

/// Moves the open run, if any, to the finished runs.
fn close_run(current: &mut Option<OpenRun>, runs: &mut Vec<EqRun>) {
    if let Some(open) = current.take() {
        runs.push(open.run);
    }
}

#[cfg(test)]
#[allow(clippy::min_ident_chars, clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    fn vocab(words: &[&str]) -> HashSet<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn soft_hyphens_are_removed() {
        let v = vocab(&["multi", "head", "multi-head", "self", "attention", "information"]);
        let cfg = TextConfig::default();
        // Unknown fragments: a plain word break.
        assert!(remove_hyphen("dot-product (multi-", "plicative) attention", &v, &cfg));
        // The joined word is known.
        assert!(remove_hyphen("more in-", "formation is needed", &v, &cfg));
        // Both halves are words of their own and the compound is not known.
        assert!(!remove_hyphen("layers of self-", "attention are used", &v, &cfg));
        // A compound seen elsewhere in the document keeps its hyphen.
        assert!(!remove_hyphen("we use multi-", "head attention", &v, &cfg));
        // Capitalised or non-alphabetic continuations keep the hyphen.
        assert!(!remove_hyphen("the English-", "German task", &v, &cfg));
        assert!(!remove_hyphen("pages 10-", "20 of the", &v, &cfg));
        // Part of a longer hyphenated compound.
        assert!(!remove_hyphen("a state-of-", "the-art model", &v, &cfg));
    }
}
