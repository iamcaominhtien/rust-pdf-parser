//! Building the text of blocks: styled spans, de-hyphenation, equation runs and table cells.
//!
//! [`TextBuilder`] turns the glyph lines of a block into the styled
//! [`Span`]s of the output model. Line-end hyphens are removed when the
//! document's own vocabulary (see [`build_vocab`]) says they are soft.

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

/// Vertical bars of formulas (absolute value, norm, "such that"). TeX builds
/// the tall ones from pieces of an extension font; each piece is drawn on its
/// own, fitted to the box its ink has in the PDF, because a bar of a text
/// font is as tall as the font says, whatever the piece it stands for.
const BAR_GLYPHS: [&str; 4] = ["|", "\u{2223}", "\u{2225}", "\u{2016}"];

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

/// The reading order of a line's glyphs: left to right, except that a
/// superscript and a subscript set one above the other ("410^{+160}_{-180}")
/// are read one after the other, not interleaved digit by digit.
fn stacked_script_order(glyphs: &[LineGlyph]) -> Vec<usize> {
    let mut order: Vec<usize> = Vec::with_capacity(glyphs.len());
    let mut i = 0;
    while i < glyphs.len() {
        if glyphs[i].script == 0 {
            order.push(i);
            i += 1;
            continue;
        }
        // A run of script glyphs: superscripts first when both kinds overlap.
        let start = i;
        while i < glyphs.len() && glyphs[i].script != 0 {
            i += 1;
        }
        let run = start..i;
        let span = |sign: i8| {
            run.clone()
                .filter(|&k| glyphs[k].script == sign)
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), k| {
                    (lo.min(glyphs[k].x0), hi.max(glyphs[k].x1))
                })
        };
        let (up, down) = (span(1), span(-1));
        let stacked = up.0 < down.1 && down.0 < up.1;
        if stacked {
            order.extend(run.clone().filter(|&k| glyphs[k].script > 0));
            order.extend(run.filter(|&k| glyphs[k].script < 0));
        } else {
            order.extend(run);
        }
    }
    order
}

impl TextBuilder<'_> {
    /// Styled text of a single line, exactly as it appears.
    pub(crate) fn line_spans(&self, line: &Line) -> Vec<Span> {
        let mut spans: Vec<Span> = Vec::new();
        for (glyph_idx, &index) in stacked_script_order(&line.glyphs).iter().enumerate() {
            let glyph = &line.glyphs[index];
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
        // A URL broken across lines goes on without a space. A hyphen at
        // the break is part of it ("py-faster-" / "rcnn"), unless TeX
        // hyphenated a word there: the joined word is used elsewhere in
        // the paper ("ope-" / "nai.com", and the paper says OpenAI).
        if url_continues(text, next) {
            if text.ends_with('-') && url_hyphen_is_soft(text, next, self.vocab) {
                if let Some(last) = spans.last_mut() {
                    last.text.pop();
                    if last.text.is_empty() {
                        spans.pop();
                    }
                }
                return true;
            }
            return false;
        }
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
    /// font size, centred far from the math axis, a radical sign or a
    /// vertical bar ([`BAR_GLYPHS`]).
    fn stands_alone(&self, glyph: &LineGlyph) -> bool {
        let axis = glyph.oy - self.cfg.math_axis_height_em * glyph.size;
        glyph.ink.height() > self.cfg.tall_glyph_min_height_em * glyph.size
            || (glyph.ink.cy() - axis).abs() > self.cfg.off_axis_min_offset_em * glyph.size
            || glyph.text == "\u{221A}"
            || BAR_GLYPHS.contains(&glyph.text.as_str())
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

/// Whether a line ends inside a URL that the next line goes on with: the
/// last word of `text` is a URL ("https://…", "www.…") and ends where TeX
/// breaks URLs ("faster_" / "rcnn", "py-faster-" / "rcnn"). After a period
/// the next line must go on in lower case or with a digit ("github." /
/// "com/…"), not start a sentence.
pub(crate) fn url_continues(text: &str, next: &str) -> bool {
    let Some(last) = text.split_whitespace().last() else {
        return false;
    };
    let last = last.trim_start_matches(['(', '[', '<', '"', '\u{201c}']);
    if !(last.contains("://") || last.starts_with("www.")) {
        return false;
    }
    let Some(first) = next.trim_start().chars().next() else {
        return false;
    };
    if next.starts_with(char::is_whitespace) {
        return false;
    }
    match last.chars().last() {
        Some('_' | '-' | '/' | '?' | '=' | '&' | '#' | '~' | '%' | ':') => true,
        Some('.') => first.is_lowercase() || first.is_ascii_digit(),
        _ => false,
    }
}

/// Whether the hyphen ending `text`, inside a URL, was put there by
/// hyphenation: the letters on both sides of it make a word of the
/// document.
fn url_hyphen_is_soft(text: &str, next: &str, vocab: &HashSet<String>) -> bool {
    let left: String = text
        .chars()
        .rev()
        .skip(1)
        .take_while(|ch| ch.is_alphabetic())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let right: String = next.chars().take_while(|ch| ch.is_alphabetic()).collect();
    !left.is_empty() && !right.is_empty() && vocab.contains(&format!("{left}{right}").to_lowercase())
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
        // "PAS-\nCAL": TeX broke a word in capitals the paper uses whole.
        let capitals = |part: &str| part.chars().all(char::is_uppercase);
        return trailing.chars().count() >= 2
            && leading.chars().count() >= 2
            && capitals(&trailing)
            && capitals(&leading)
            && vocab.contains(&format!("{trailing}{leading}").to_lowercase());
    }
    // "state-of-\nthe-art": the left part is itself a hyphenated compound.
    let before: Vec<char> = left.chars().rev().skip(1 + trailing.chars().count()).take(1).collect();
    if before.first() == Some(&'-') {
        return false;
    }
    // "CNN-\nbased", "RoI-\nbased": TeX never breaks an acronym (two
    // capitals, the last letter one of them); the hyphen is real.
    if trailing.chars().filter(|ch| ch.is_uppercase()).count() >= 2
        && trailing.chars().last().is_some_and(char::is_uppercase)
    {
        return false;
    }
    let (left_part, right_part) = (trailing.to_lowercase(), leading.to_lowercase());
    if vocab.contains(&format!("{left_part}{right_part}")) {
        return true;
    }
    if vocab.contains(&format!("{left_part}-{right_part}")) {
        return false;
    }
    // "sequence-\naligned": both parts are words of the paper, the right one
    // perhaps with an inflection ("aligned" where the paper says "align").
    let known = |word: &str| {
        vocab.contains(word)
            || ["ed", "d", "ing", "s", "es"].iter().any(|suffix| {
                word.strip_suffix(suffix).is_some_and(|stem| {
                    stem.chars().count() >= cfg.compound_part_min_chars_count && vocab.contains(stem)
                })
            })
    };
    !(left_part.chars().count() >= cfg.compound_part_min_chars_count
        && vocab.contains(&left_part)
        && known(&right_part))
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
    fn urls_broken_across_lines_go_on() {
        assert!(url_continues(
            "available at https://github.com/shaoqingren/faster_",
            "rcnn (in MATLAB) and"
        ));
        assert!(url_continues(
            "see https://github.com/rbgirshick/py-faster-",
            "rcnn (in Python)."
        ));
        assert!(url_continues("at www.robots.ox.ac.", "uk/~vgg"));
        assert!(!url_continues("results at https://example.com/page.html.", "We then"));
        assert!(!url_continues("the dot-product (multi-", "plicative) attention"));
        assert!(!url_continues("https://example.com/a", "and more"));
        let vocab: HashSet<String> = ["openai", "faster", "rcnn"].iter().map(|w| w.to_string()).collect();
        assert!(url_hyphen_is_soft("https://ope-", "nai.com/blog", &vocab));
        assert!(!url_hyphen_is_soft(
            "https://github.com/rbgirshick/py-faster-",
            "rcnn",
            &vocab
        ));
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
