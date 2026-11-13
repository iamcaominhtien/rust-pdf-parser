//! Rows of a column -> blocks: paragraphs, display equations, footnotes.
//!
//! Each leaf of the reading order (a column piece) is turned into blocks in
//! four steps: its rows are gathered (tiny orphan rows joined to their
//! neighbours), each row is classified as text, equation or float by a
//! sequence of rules, the footnote area at the bottom is found, and the text
//! rows are grouped into paragraphs.

use super::lines::{LinesConfig, merge_row};
use super::order::{ItemRef, Leaf};
use super::page::{DocStats, PageData};
use crate::backend::FontInfo;
use crate::common::geom::Rect;
use crate::model::Line;
use crate::model::Loc;
use regex::Regex;
use std::sync::LazyLock;

/// An equation number at the right margin: "(3)", "(A.2)", "(4.1b)".
static EQNO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\((?:[A-Z]\.?)?\d+(?:\.\d+)*[a-z]?\)$").unwrap());
/// A bullet at the start of a line.
pub static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[•◦▪■–∗⋆·‣▶►○●◆\-]\s").unwrap());
/// An enumeration label at the start of a line.
pub static NUM_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\(\d{1,2}\)|\([a-z]\)|\([ivx]{1,4}\)|\d{1,2}[.)]|[a-z]\))\s").unwrap());
/// Any list item label at the start of a line: a bullet or an enumeration
/// label (the union of [`BULLET`] and [`NUM_LABEL`]).
pub static LIST_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[•◦▪■–∗⋆·‣▶►○●◆\-]\s|\(\d{1,2}\)\s|\([a-z]\)\s|\([ivx]{1,4}\)\s|\d{1,2}[.)]\s|[a-z]\)\s)").unwrap()
});

/// The equation rules for rows without an equation number, tried in order.
const EQUATION_RULES: [EquationRule; 4] = [displayed_formula, symbol_row, centred_relation, stacked_formula];

/// The paragraph break rules, tried in order (centred groups never break).
const PARAGRAPH_RULES: [ParagraphRule; 4] = [
    starts_with_marker,
    new_enumeration_item,
    follows_short_line,
    indent_changes,
];

/// Tuning parameters of block building. Part of [`LayoutConfig`](crate::config::LayoutConfig).
///
/// Lengths are in PDF points (`_pt`), in multiples of a font size (`_em`:
/// the body size for leaf-level measures, else the row's or line's own
/// size) or in multiples of the document's leading (`_leading`); `_ratio`
/// fields are fractions between 0 and 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlocksConfig {
    // --- gathering rows ---
    /// A row with at most this many characters may be an orphan (a piece of
    /// inline math split off its line); the row it joins has more.
    pub orphan_max_chars: usize,
    /// An orphan row is narrower than this many body sizes.
    pub orphan_max_width_em: f32,
    /// With at least this many text rows, the right edge of the leaf is a
    /// high percentile of the row ends (robust against a few overlong
    /// rows); with fewer, it is the rightmost end.
    pub right_percentile_min_rows: usize,
    /// The percentile of row ends taken as the right edge of the leaf.
    pub right_edge_percentile_ratio: f32,

    // --- equation rows ---
    /// A row is isolated (white space around it, as around a displayed
    /// formula) when the ink gaps to the rows above and below exceed this
    /// many body sizes.
    pub isolation_gap_em: f32,
    /// An equation number ends within this many body sizes of the leaf's
    /// right edge.
    pub eqno_margin_em: f32,
    /// A row is centred when its centre is within this fraction of the
    /// leaf width of the leaf's centre ...
    pub centered_tol_ratio: f32,
    /// ... (but at least this) ...
    pub centered_min_tol_pt: f32,
    /// ... and it is indented by at least this many sizes on both sides.
    pub centered_min_margin_em: f32,
    /// A row spans several baselines (a fraction, a stacked formula) when
    /// its baselines differ by more than this many sizes.
    pub multi_baseline_em: f32,
    /// A row with an equation number is an equation when less than this
    /// fraction of its characters are prose.
    pub numbered_max_prose_ratio: f32,
    /// A displayed formula has at least this fraction of math characters...
    pub display_min_math_ratio: f32,
    /// ... at most this fraction of prose ...
    pub display_max_prose_ratio: f32,
    /// ... and is centred or indented by at least this many sizes.
    pub display_min_indent_em: f32,
    /// A row of almost nothing but symbols has at least this fraction of
    /// math characters ...
    pub symbolic_min_math_ratio: f32,
    /// ... at most this fraction of prose ...
    pub symbolic_max_prose_ratio: f32,
    /// ... at least this many characters ...
    pub symbolic_min_chars: usize,
    /// ... and is indented by at least this many sizes (or isolated).
    pub symbolic_min_indent_em: f32,
    /// A centred, isolated formula made mostly of operator names has at
    /// least this fraction of math characters ...
    pub operator_min_math_ratio: f32,
    /// ... and at most this fraction of prose (plus a relation sign).
    pub operator_max_prose_ratio: f32,
    /// A row spanning several baselines is a formula with at least this
    /// fraction of math characters ...
    pub stacked_min_math_ratio: f32,
    /// ... at most this fraction of prose ...
    pub stacked_max_prose_ratio: f32,
    /// ... and an indent of at least this many sizes on both sides.
    pub stacked_min_margin_em: f32,
    /// A weak equation candidate with a relation sign is indented by at
    /// least this many sizes on both sides ...
    pub weak_min_margin_em: f32,
    /// ... has at most this many characters ...
    pub weak_max_chars: usize,
    /// ... and at most this fraction of prose.
    pub weak_max_prose_ratio: f32,
    /// An indented row of at most this many characters and no prose (a
    /// numerator such as "1", a limit) is a weak candidate too.
    pub weak_tiny_max_chars: usize,
    /// A weak candidate indented by less than this many sizes, right below
    /// a prose row, is the wrapped end of that paragraph: it is not
    /// promoted.
    pub weak_flush_max_indent_em: f32,
    /// Without a relation sign, a weak candidate has at least this fraction
    /// of math characters ...
    pub weak_min_math_ratio: f32,
    /// ... and at most this fraction of prose.
    pub weak_math_max_prose_ratio: f32,
    /// A weak candidate is promoted to an equation when its ink gap to an
    /// adjacent equation row is less than this many leadings.
    pub promote_max_gap_leading: f32,
    /// Adjacent equation rows closer than this many leadings are merged into
    /// one equation (unless both are numbered).
    pub merge_max_gap_leading: f32,

    // --- footnotes ---
    /// Footnote text is at least this much smaller than the body size.
    pub footnote_size_drop_pt: f32,
    /// A footnote rule is at least this long ...
    pub footnote_rule_min_width_pt: f32,
    /// ... and at most this fraction of the leaf width ...
    pub footnote_rule_max_width_ratio: f32,
    /// ... (with the leaf counted as at least this wide) ...
    pub footnote_rule_min_leaf_pt: f32,
    /// ... and starts within this of the leaf's left edge.
    pub footnote_rule_align_pt: f32,
    /// Text rows starting less than this above the rule count as below it.
    pub footnote_rule_tol_pt: f32,
    /// Without a rule, the footnote area is a trailing run of lines at
    /// least this much smaller than the body size ...
    pub footnote_run_size_drop_pt: f32,
    /// ... introduced by a footnote mark, more than this many leadings
    /// (scaled to the line's size) below the row above.
    pub footnote_gap_leading: f32,

    // --- paragraphs ---
    /// The lines of a centred group have their centres within this of the
    /// median centre ...
    pub centered_group_tol_pt: f32,
    /// ... and at least one starts more than this right of the group's left
    /// edge (otherwise it is justified text).
    pub centered_group_min_offset_pt: f32,
    /// Width of the space before a word, in sizes, when testing whether the
    /// next word would have fit on the previous line.
    pub para_word_space_em: f32,
    /// A line ends short (ending its paragraph) when the next word plus a
    /// space would have fit before the group's right edge minus this.
    pub para_short_line_slack_pt: f32,
    /// A numbered line starts a new item when it starts within this many
    /// sizes of the paragraph's first line.
    pub para_label_align_em: f32,
    /// A line starting more than this many sizes left or right of the
    /// paragraph's other continuation lines starts a new paragraph.
    pub para_indent_change_em: f32,
    /// Two text rows belong to different blocks when their baseline gap
    /// exceeds this many expected leadings ...
    pub block_gap_leading: f32,
    /// ... plus this slack ...
    pub block_gap_slack_pt: f32,
    /// ... and their ink is more than this many sizes apart.
    pub block_ink_gap_em: f32,
    /// Two text rows whose sizes differ by more than this belong to
    /// different blocks.
    pub block_size_change_pt: f32,
    /// A change between bold and regular separates blocks only when both
    /// rows have more than this many characters ...
    pub bold_flip_min_chars: usize,
    /// ... and either the upper row is bold or the gap exceeds this many
    /// expected leadings.
    pub bold_flip_gap_leading: f32,
    /// A row is bold when at least this fraction of its glyphs are bold.
    pub bold_dominant_ratio: f32,
}

impl Default for BlocksConfig {
    fn default() -> Self {
        Self {
            orphan_max_chars: 4,
            orphan_max_width_em: 3.5,
            right_percentile_min_rows: 8,
            right_edge_percentile_ratio: 0.92,
            isolation_gap_em: 0.55,
            eqno_margin_em: 2.5,
            centered_tol_ratio: 0.03,
            centered_min_tol_pt: 4.0,
            centered_min_margin_em: 1.2,
            multi_baseline_em: 0.25,
            numbered_max_prose_ratio: 0.6,
            display_min_math_ratio: 0.25,
            display_max_prose_ratio: 0.5,
            display_min_indent_em: 3.0,
            symbolic_min_math_ratio: 0.45,
            symbolic_max_prose_ratio: 0.15,
            symbolic_min_chars: 6,
            symbolic_min_indent_em: 1.0,
            operator_min_math_ratio: 0.08,
            operator_max_prose_ratio: 0.8,
            stacked_min_math_ratio: 0.3,
            stacked_max_prose_ratio: 0.3,
            stacked_min_margin_em: 1.0,
            weak_min_margin_em: 1.2,
            weak_max_chars: 110,
            weak_max_prose_ratio: 0.85,
            weak_tiny_max_chars: 3,
            weak_flush_max_indent_em: 0.5,
            weak_min_math_ratio: 0.2,
            weak_math_max_prose_ratio: 0.3,
            promote_max_gap_leading: 0.9,
            merge_max_gap_leading: 0.9,
            footnote_size_drop_pt: 0.4,
            footnote_rule_min_width_pt: 20.0,
            footnote_rule_max_width_ratio: 0.62,
            footnote_rule_min_leaf_pt: 120.0,
            footnote_rule_align_pt: 8.0,
            footnote_rule_tol_pt: 2.0,
            footnote_run_size_drop_pt: 0.9,
            footnote_gap_leading: 1.25,
            centered_group_tol_pt: 2.5,
            centered_group_min_offset_pt: 3.0,
            para_word_space_em: 0.25,
            para_short_line_slack_pt: 1.0,
            para_label_align_em: 0.6,
            para_indent_change_em: 0.6,
            block_gap_leading: 1.25,
            block_gap_slack_pt: 1.5,
            block_ink_gap_em: 0.45,
            block_size_change_pt: 0.6,
            bold_flip_min_chars: 2,
            bold_flip_gap_leading: 1.1,
            bold_dominant_ratio: 0.95,
        }
    }
}

/// A block of running text: a paragraph, a list item, a heading.
#[derive(Debug, Clone)]
pub struct TextBlock {
    /// Its lines, top to bottom.
    pub lines: Vec<Line>,
    /// Where it is on the page.
    pub loc: Loc,
    /// Left edge of the surrounding text group.
    pub left: f32,
    /// Right edge of the surrounding text group.
    pub right: f32,
    /// The surrounding group is centred text.
    pub centered: bool,
    /// Baseline distance to the previous row of the leaf.
    pub gap_before: f32,
    /// Union of the line boxes.
    pub bbox: Rect,
}

/// A displayed equation.
#[derive(Debug, Clone)]
pub struct EquationBlock {
    /// The pieces of the equation, as lines.
    pub lines: Vec<Line>,
    /// Ink extent of the equation without its number.
    pub bbox: Rect,
    /// The equation number, e.g. "(3)".
    pub number: Option<String>,
    /// The line holding the equation number, if any.
    pub number_line: Option<Line>,
    /// Where it is on the page.
    pub loc: Loc,
}

/// A block of a leaf, in reading order.
#[derive(Debug, Clone)]
pub enum RawBlock {
    /// Running text.
    Text(TextBlock),
    /// A displayed equation.
    Equation(EquationBlock),
    /// The float with index `idx` on the page sits here.
    Float {
        /// Index of the float in the page's floats.
        idx: usize,
        /// Where it is on the page.
        loc: Loc,
    },
    /// A footnote.
    Footnote(TextBlock),
}

/// Character counts of a row used to tell formulas from prose.
struct RowStats {
    /// All characters.
    n_chars: usize,
    /// Characters in a math font or that are math symbols.
    math_chars: usize,
    /// Characters in runs of at least four letters (words).
    prose_chars: usize,
}

/// A row of a leaf after classification.
enum Row {
    /// A line of text (the row's lines merged).
    Text(Line),
    /// A displayed equation.
    Equation {
        /// Its pieces.
        lines: Vec<Line>,
        /// Its ink extent without the number.
        bbox: Rect,
        /// Its number, if any.
        number: Option<String>,
        /// The line holding the number.
        number_line: Option<Line>,
    },
    /// A float (index into the page's floats).
    Float(usize),
}

/// A row of a leaf as gathered from the reading order, before
/// classification.
struct RowDraft {
    /// The lines of the row.
    lines: Vec<Line>,
    /// The float in the row, if any.
    float: Option<usize>,
    /// Union of the line boxes.
    bbox: Rect,
}

/// A text row that is not an equation on its own but looks like part of
/// one: its lines and ink extent. It is promoted when it sits next to an
/// equation row.
/// A weak equation candidate: its lines, its ink, and whether it is flush
/// with the left edge of the leaf.
type WeakEquation = (Vec<Line>, Rect, bool);

/// The horizontal extent of the text of a leaf.
struct LeafExtent {
    /// Leftmost start of a text row.
    left: f32,
    /// Right edge of the text (a high percentile of the row ends).
    right: f32,
    /// `right - left`, at least 1.
    width: f32,
    /// Centre between `left` and `right`.
    center_x: f32,
}

/// The measures of one row that the equation rules test.
struct RowFeatures {
    /// The row ends with an equation number.
    numbered: bool,
    /// Number of characters (without the number).
    n_chars: usize,
    /// Fraction of math characters.
    math_frac: f32,
    /// Fraction of prose characters.
    prose_frac: f32,
    /// Font size of the row (the largest, at least 1).
    em: f32,
    /// Distance from the leaf's left edge.
    left_indent: f32,
    /// Distance to the leaf's right edge.
    right_slack: f32,
    /// The row is centred in the leaf.
    centered: bool,
    /// There is white space above and below the row.
    isolated: bool,
    /// The row spans several baselines.
    multi: bool,
    /// The row contains a relation sign ("=", "≤", ...).
    has_relation: bool,
}

/// A rule of the equation classification: whether a row is a display
/// equation for one reason.
type EquationRule = fn(&RowFeatures, &BlocksConfig) -> bool;

/// A candidate line for the end of a paragraph, as the paragraph rules see
/// it.
struct ParagraphBreak<'a> {
    /// The paragraph so far (lines and gaps before them).
    para: &'a [(Line, f32)],
    /// The candidate next line.
    line: &'a Line,
    /// The text of `line`.
    text: &'a str,
    /// Right edge of the text group.
    right: f32,
    /// The group is in the footnote area.
    footnote: bool,
}

/// A rule of the paragraph splitting: whether a line starts a new
/// paragraph for one reason.
type ParagraphRule = fn(&ParagraphBreak, &BlocksConfig) -> bool;

/// Everything the steps of [`build_blocks`] read.
#[derive(Clone, Copy)]
struct BlockContext<'a> {
    /// The page.
    page: &'a PageData,
    /// Document-wide font statistics.
    stats: &'a DocStats,
    /// The document's fonts.
    fonts: &'a [FontInfo],
    /// The thresholds.
    cfg: &'a BlocksConfig,
    /// Thresholds of line building, for rows merged into one line.
    lines_cfg: &'a LinesConfig,
}

/// Turns the rows of every leaf of a page into blocks, in reading order:
/// text blocks (paragraphs, list items), display equations, floats, and the
/// footnotes at the bottom of a leaf.
pub fn build_blocks(
    page: &PageData,
    leaves: &[Leaf],
    stats: &DocStats,
    fonts: &[FontInfo],
    cfg: &BlocksConfig,
    lines_cfg: &LinesConfig,
) -> Vec<RawBlock> {
    let ctx = BlockContext {
        page,
        stats,
        fonts,
        cfg,
        lines_cfg,
    };
    let mut out = Vec::new();
    for (leaf_idx, leaf) in leaves.iter().enumerate() {
        let loc = Loc {
            page: page.index,
            zone: leaf.zone,
            zone_cols: leaf.zone_cols,
            col: leaf.col,
            leaf: leaf_idx,
        };
        let mut drafts = gather_rows(page, leaf);
        join_orphan_rows(&mut drafts, stats, cfg);
        let extent = LeafExtent::measure(&drafts, leaf, cfg);
        let (mut rows, mut weak) = classify_rows(ctx, drafts, &extent);
        promote_weak_equations(&mut rows, &mut weak, stats, cfg);
        let rows = merge_equation_rows(rows, stats, cfg);
        let fn_start = footnote_start(ctx, leaf, &rows, &extent);
        emit_blocks(ctx, rows, fn_start, loc, &mut out);
    }
    out
}

impl LeafExtent {
    /// Measures the text rows (rows without a float) of a leaf. The right
    /// edge falls back to the leaf's box when the percentile has no data.
    fn measure(drafts: &[RowDraft], leaf: &Leaf, cfg: &BlocksConfig) -> Self {
        let text_x1: Vec<f32> = drafts
            .iter()
            .filter(|draft| draft.float.is_none())
            .map(|draft| draft.bbox.x1)
            .collect();
        let left = drafts
            .iter()
            .filter(|draft| draft.float.is_none())
            .map(|draft| draft.bbox.x0)
            .fold(f32::INFINITY, f32::min);
        let right = if text_x1.len() >= cfg.right_percentile_min_rows {
            percentile(text_x1, cfg.right_edge_percentile_ratio, leaf.bbox.x1)
        } else {
            text_x1.iter().copied().fold(f32::NEG_INFINITY, f32::max)
        };
        Self {
            left,
            right,
            width: (right - left).max(1.0),
            center_x: (left + right) * 0.5,
        }
    }
}

/// Whether `ch` is a math symbol (operators, relations, arrows, Greek and
/// mathematical alphanumerics).
fn is_math_char(ch: char) -> bool {
    matches!(
        ch,
        '=' | '+'
            | '−'
            | '×'
            | '÷'
            | '·'
            | '∑'
            | '∏'
            | '∫'
            | '√'
            | '∞'
            | '≤'
            | '≥'
            | '≠'
            | '≈'
            | '∈'
            | '∉'
            | '⊂'
            | '⊆'
            | '∪'
            | '∩'
            | '∧'
            | '∨'
            | '¬'
            | '→'
            | '←'
            | '↔'
            | '⇒'
            | '⇔'
            | '∀'
            | '∃'
            | '∂'
            | '∇'
            | '∝'
            | '∼'
            | '≡'
            | '⟨'
            | '⟩'
            | '‖'
            | '|'
            | '<'
            | '>'
            | '±'
            | '∓'
            | '⊕'
            | '⊗'
            | '∘'
            | '′'
            | '⊤'
            | '⊥'
            | '∥'
            | '≜'
            | '≪'
            | '≫'
            | '↦'
            | '∖'
    ) || ('\u{0391}'..='\u{03C9}').contains(&ch)
        || ('\u{1D400}'..='\u{1D7FF}').contains(&ch)
}

/// Whether a glyph's text is a relation sign, the mark of a formula.
fn is_relation(text: &str) -> bool {
    matches!(text, "=" | "≤" | "≥" | "≈" | "∈" | "→" | "∝" | "≡" | "<" | ">")
}

/// Counts the characters, math characters and prose characters (runs of at
/// least four letters without a space) of a row.
fn row_stats(lines: &[&Line], fonts: &[FontInfo]) -> RowStats {
    let mut stats = RowStats {
        n_chars: 0,
        math_chars: 0,
        prose_chars: 0,
    };
    for line in lines {
        let mut run = 0usize;
        for glyph in &line.glyphs {
            let font = &fonts[glyph.font as usize];
            let nchars = glyph.text.chars().count();
            stats.n_chars += nchars;
            let mathy = font.math || glyph.text.chars().any(is_math_char);
            if mathy {
                stats.math_chars += nchars;
            }
            let alpha = !mathy && glyph.script == 0 && glyph.text.chars().all(|ch| ch.is_alphabetic());
            if alpha && !glyph.space_before {
                run += nchars;
            } else {
                if run >= 4 {
                    stats.prose_chars += run;
                }
                run = if alpha { nchars } else { 0 };
            }
        }
        if run >= 4 {
            stats.prose_chars += run;
        }
    }
    stats
}

/// The value at quantile `quantile` (0-1, nearest rank) of `values`, or
/// `default` when there are none.
fn percentile(mut values: Vec<f32>, quantile: f32, default: f32) -> f32 {
    if values.is_empty() {
        return default;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let idx = ((values.len() - 1) as f32 * quantile).round() as usize;
    values[idx]
}

/// Whether the line ends with a hyphen (a word continues on the next line).
fn ends_with_hyphen(line: &Line) -> bool {
    line.glyphs.last().is_some_and(|glyph| glyph.text.ends_with('-'))
}

/// Whether (nearly) the whole line is set in bold.
fn bold_dominant(line: &Line, fonts: &[FontInfo], cfg: &BlocksConfig) -> bool {
    line.font_frac(fonts, |font| font.bold) >= cfg.bold_dominant_ratio
}

/// Whether the line starts with a footnote mark: a superscript or a
/// footnote symbol.
fn footnote_marked(line: &Line) -> bool {
    line.starts_with_superscript()
        || line
            .glyphs
            .first()
            .is_some_and(|glyph| matches!(glyph.text.as_str(), "∗" | "†" | "‡" | "§" | "¶" | "*"))
}

/// Total number of characters of the row's lines.
fn row_chars(draft: &RowDraft) -> usize {
    draft.lines.iter().map(|line| line.char_count()).sum::<usize>()
}

/// Collects the rows of a leaf from the reading order: the lines of each
/// row (cloned from the page) and the float it holds, if any.
fn gather_rows(page: &PageData, leaf: &Leaf) -> Vec<RowDraft> {
    let mut drafts: Vec<RowDraft> = Vec::new();
    for row in &leaf.rows {
        let mut lines = Vec::new();
        let mut float = None;
        for item in row {
            match *item {
                ItemRef::Line(i) => lines.push(page.lines[i].clone()),
                ItemRef::Float(i) => float = Some(i),
            }
        }
        let bbox = lines.iter().fold(Rect::EMPTY, |acc, line| acc.union(&line.bbox));
        drafts.push(RowDraft { lines, float, bbox });
    }
    drafts
}

/// Tiny orphan rows (pieces of inline math) join the row they overlap: the
/// previous or next row, whichever is vertically closer.
fn join_orphan_rows(drafts: &mut Vec<RowDraft>, stats: &DocStats, cfg: &BlocksConfig) {
    let mut i = 0;
    while i < drafts.len() {
        let tiny = drafts[i].float.is_none()
            && row_chars(&drafts[i]) <= cfg.orphan_max_chars
            && drafts[i].bbox.width() < cfg.orphan_max_width_em * stats.body_size;
        if tiny && let Some(target) = orphan_target(drafts, i, stats, cfg) {
            let row = drafts.remove(i);
            let target = if target > i { target - 1 } else { target };
            drafts[target].lines.extend(row.lines);
            drafts[target].bbox = drafts[target].bbox.union(&row.bbox);
            continue;
        }
        i += 1;
    }
}

/// The index of the neighbour row that the orphan row `idx` joins: a text
/// row with more characters that overlaps it vertically and covers it
/// horizontally (within a body size). The closer one wins when both
/// neighbours qualify; `None` when neither does.
fn orphan_target(drafts: &[RowDraft], idx: usize, stats: &DocStats, cfg: &BlocksConfig) -> Option<usize> {
    let orphan = drafts[idx].bbox;
    let accepts = |other: &RowDraft| {
        other.float.is_none()
            && other.bbox.y_overlap(&orphan) > 0.0
            && orphan.x0 >= other.bbox.x0 - stats.body_size
            && orphan.x1 <= other.bbox.x1 + stats.body_size
            && row_chars(other) > cfg.orphan_max_chars
    };
    let prev_ok = idx > 0 && accepts(&drafts[idx - 1]);
    let next_ok = idx + 1 < drafts.len() && accepts(&drafts[idx + 1]);
    match (prev_ok, next_ok) {
        (true, true) => {
            let dist_prev = (drafts[idx - 1].bbox.cy() - orphan.cy()).abs();
            let dist_next = (drafts[idx + 1].bbox.cy() - orphan.cy()).abs();
            Some(if dist_prev <= dist_next { idx - 1 } else { idx + 1 })
        }
        (true, false) => Some(idx - 1),
        (false, true) => Some(idx + 1),
        _ => None,
    }
}

/// Classifies every row of a leaf as float, equation or text. Returns the
/// rows and, parallel to them, the weak equation candidates among the text
/// rows (`None` for every other row). A row holding a float and lines
/// yields a float row followed by a row for the lines; empty rows yield
/// nothing.
fn classify_rows(
    ctx: BlockContext,
    drafts: Vec<RowDraft>,
    extent: &LeafExtent,
) -> (Vec<Row>, Vec<Option<WeakEquation>>) {
    let mut rows: Vec<Row> = Vec::new();
    let mut weak: Vec<Option<WeakEquation>> = Vec::new();
    let ink_boxes: Vec<Rect> = drafts
        .iter()
        .map(|draft| draft.lines.iter().fold(Rect::EMPTY, |acc, line| acc.union(&line.ink)))
        .collect();
    for (draft_idx, draft) in drafts.into_iter().enumerate() {
        let isolated = is_isolated(&ink_boxes, draft_idx, ctx.stats, ctx.cfg);
        if let Some(float) = draft.float {
            rows.push(Row::Float(float));
            weak.push(None);
        }
        if draft.lines.is_empty() {
            continue;
        }
        let (row, weak_eq) = classify_text_row(ctx, draft.lines, extent, isolated);
        weak.push(weak_eq);
        rows.push(row);
    }
    (rows, weak)
}

/// Whether row `idx` has white space above and below it, as around a
/// displayed formula. The first and last rows, and neighbours without ink,
/// count as white space.
fn is_isolated(ink_boxes: &[Rect], idx: usize, stats: &DocStats, cfg: &BlocksConfig) -> bool {
    let ink = ink_boxes[idx];
    let above = idx == 0
        || ink_boxes[idx - 1].is_unset()
        || ink.y0 - ink_boxes[idx - 1].y1 > cfg.isolation_gap_em * stats.body_size;
    let below = idx + 1 >= ink_boxes.len()
        || ink_boxes[idx + 1].is_unset()
        || ink_boxes[idx + 1].y0 - ink.y1 > cfg.isolation_gap_em * stats.body_size;
    above && below
}

/// Classifies the lines of one row (not empty) as an equation or a text
/// row. A text row also returns its lines and ink as a weak equation
/// candidate when it looks formula-like.
fn classify_text_row(
    ctx: BlockContext,
    mut lines: Vec<Line>,
    extent: &LeafExtent,
    isolated: bool,
) -> (Row, Option<WeakEquation>) {
    lines.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
    let number = equation_number(&lines, extent, ctx.stats, ctx.cfg);
    let (is_eq, weak_eq, ink, flush) = {
        let content: Vec<&Line> = if number.is_some() {
            lines[..lines.len() - 1].iter().collect()
        } else {
            lines.iter().collect()
        };
        let features = RowFeatures::measure(ctx, &content, extent, isolated, number.is_some());
        let is_eq = is_equation(&features, ctx.cfg);
        // A formula-like line that did not qualify on its own; it is
        // promoted later if it sits right next to an equation row.
        let weak_eq = !is_eq && weak_equation(&features, ctx.cfg);
        let flush = features.left_indent < ctx.cfg.weak_flush_max_indent_em * features.em;
        let ink = content.iter().fold(Rect::EMPTY, |acc, line| acc.union(&line.ink));
        (is_eq, weak_eq, ink, flush)
    };
    if is_eq {
        let number_line = if number.is_some() { lines.pop() } else { None };
        let row = Row::Equation {
            lines,
            bbox: ink,
            number,
            number_line,
        };
        (row, None)
    } else {
        let weak = weak_eq.then_some((lines.clone(), ink, flush));
        (Row::Text(merge_row(ctx.page.index, lines, ctx.lines_cfg)), weak)
    }
}

/// The equation number of a row (lines sorted left to right): its last line
/// when the row has at least two lines and that line is a number like "(3)"
/// at the right margin. Returned without white space.
fn equation_number(lines: &[Line], extent: &LeafExtent, stats: &DocStats, cfg: &BlocksConfig) -> Option<String> {
    if lines.len() < 2 {
        return None;
    }
    let last = lines.last().unwrap();
    let text: String = last.text().chars().filter(|ch| !ch.is_whitespace()).collect();
    (EQNO.is_match(&text) && last.bbox.x1 >= extent.right - cfg.eqno_margin_em * stats.body_size).then_some(text)
}

impl RowFeatures {
    /// Measures the `content` lines of a row (without its equation number)
    /// against the leaf's `extent`.
    fn measure(ctx: BlockContext, content: &[&Line], extent: &LeafExtent, isolated: bool, numbered: bool) -> Self {
        let cfg = ctx.cfg;
        let stats = row_stats(content, ctx.fonts);
        let content_box = content.iter().fold(Rect::EMPTY, |acc, line| acc.union(&line.bbox));
        let em = content.iter().map(|line| line.size).fold(0.0, f32::max).max(1.0);
        let left_indent = content_box.x0 - extent.left;
        let right_slack = extent.right - content_box.x1;
        let centered = (content_box.cx() - extent.center_x).abs()
            <= (cfg.centered_tol_ratio * extent.width).max(cfg.centered_min_tol_pt)
            && left_indent >= cfg.centered_min_margin_em * em
            && right_slack >= cfg.centered_min_margin_em * em;
        let multi = {
            let mut baselines: Vec<f32> = content.iter().map(|line| line.baseline).collect();
            baselines.sort_by(|a, b| a.total_cmp(b));
            baselines.last().unwrap() - baselines[0] > cfg.multi_baseline_em * em
        };
        let has_relation = content
            .iter()
            .any(|line| line.glyphs.iter().any(|glyph| is_relation(&glyph.text)));
        Self {
            numbered,
            n_chars: stats.n_chars,
            math_frac: stats.math_chars as f32 / stats.n_chars.max(1) as f32,
            prose_frac: stats.prose_chars as f32 / stats.n_chars.max(1) as f32,
            em,
            left_indent,
            right_slack,
            centered,
            isolated,
            multi,
            has_relation,
        }
    }
}

/// Whether a row is a display equation. A numbered row is one unless it is
/// mostly prose; other rows need one of the [`EQUATION_RULES`].
fn is_equation(features: &RowFeatures, cfg: &BlocksConfig) -> bool {
    if features.numbered {
        return features.prose_frac < cfg.numbered_max_prose_ratio;
    }
    EQUATION_RULES.iter().any(|rule| rule(features, cfg))
}

/// Rule: mostly math, little prose, centred or clearly indented.
fn displayed_formula(row: &RowFeatures, cfg: &BlocksConfig) -> bool {
    row.math_frac >= cfg.display_min_math_ratio
        && row.prose_frac <= cfg.display_max_prose_ratio
        && (row.centered || row.left_indent >= cfg.display_min_indent_em * row.em)
}

/// Rule: almost nothing but symbols, indented or isolated.
fn symbol_row(row: &RowFeatures, cfg: &BlocksConfig) -> bool {
    row.math_frac >= cfg.symbolic_min_math_ratio
        && row.prose_frac <= cfg.symbolic_max_prose_ratio
        && row.n_chars >= cfg.symbolic_min_chars
        && (row.left_indent >= cfg.symbolic_min_indent_em * row.em || row.isolated)
}

/// Rule: a centred, isolated formula made mostly of operator names, with a
/// relation sign.
fn centred_relation(row: &RowFeatures, cfg: &BlocksConfig) -> bool {
    row.centered
        && row.isolated
        && row.math_frac >= cfg.operator_min_math_ratio
        && row.prose_frac <= cfg.operator_max_prose_ratio
        && row.has_relation
}

/// Rule: a formula spanning several baselines (fractions, stacked limits),
/// indented on both sides.
fn stacked_formula(row: &RowFeatures, cfg: &BlocksConfig) -> bool {
    row.multi
        && row.math_frac >= cfg.stacked_min_math_ratio
        && row.prose_frac <= cfg.stacked_max_prose_ratio
        && row.left_indent >= cfg.stacked_min_margin_em * row.em
        && row.right_slack >= cfg.stacked_min_margin_em * row.em
}

/// Whether a row that is not an equation on its own looks like one line of
/// a multi-line display: an indented short row with a relation sign, or a
/// row with a fair amount of math and little prose, or a tiny indented row
/// without prose (the numerator of a fraction).
fn weak_equation(row: &RowFeatures, cfg: &BlocksConfig) -> bool {
    (row.has_relation
        && row.left_indent >= cfg.weak_min_margin_em * row.em
        && row.right_slack >= cfg.weak_min_margin_em * row.em
        && row.n_chars <= cfg.weak_max_chars
        && row.prose_frac <= cfg.weak_max_prose_ratio)
        || (row.math_frac >= cfg.weak_min_math_ratio && row.prose_frac <= cfg.weak_math_max_prose_ratio)
        || (row.n_chars <= cfg.weak_tiny_max_chars
            && row.prose_frac == 0.0
            && row.left_indent >= cfg.weak_min_margin_em * row.em
            && row.right_slack >= cfg.weak_min_margin_em * row.em)
}

/// Promotes weak candidates adjacent to an equation (lines of a multi-line
/// display where only some lines look like math), repeatedly, until no
/// more rows change.
fn promote_weak_equations(rows: &mut [Row], weak: &mut [Option<WeakEquation>], stats: &DocStats, cfg: &BlocksConfig) {
    loop {
        let mut changed = false;
        for i in 0..rows.len() {
            let Some((_, ink, flush)) = &weak[i] else { continue };
            // A flush-left row right below prose is the wrapped end of that
            // paragraph ("… for all / T ≥ 1."), not a line of the display.
            if *flush && i > 0 && matches!(rows[i - 1], Row::Text(_)) && weak[i - 1].is_none() {
                continue;
            }
            let near = |j: usize| match &rows[j] {
                Row::Equation { bbox, .. } => {
                    let gap = if bbox.cy() < ink.cy() {
                        ink.y0 - bbox.y1
                    } else {
                        bbox.y0 - ink.y1
                    };
                    gap < cfg.promote_max_gap_leading * stats.leading
                }
                _ => false,
            };
            if (i > 0 && near(i - 1)) || (i + 1 < rows.len() && near(i + 1)) {
                let (lines, bbox, _) = weak[i].take().unwrap();
                rows[i] = Row::Equation {
                    lines,
                    bbox,
                    number: None,
                    number_line: None,
                };
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// Merges vertically adjacent equation rows (multi-line equations,
/// numerators and denominators) unless both carry a number. The merged
/// equation keeps the first number; the line of a second number stays
/// among its lines so that its glyphs are drawn.
fn merge_equation_rows(rows: Vec<Row>, stats: &DocStats, cfg: &BlocksConfig) -> Vec<Row> {
    let mut merged: Vec<Row> = Vec::new();
    for row in rows {
        let Row::Equation {
            lines,
            bbox,
            number,
            number_line,
        } = row
        else {
            merged.push(row);
            continue;
        };
        if let Some(Row::Equation {
            lines: prev_lines,
            bbox: prev_bbox,
            number: prev_number,
            number_line: prev_number_line,
        }) = merged.last_mut()
        {
            let gap = bbox.y0 - prev_bbox.y1;
            let overlap = gap < 0.0;
            if overlap
                || (gap < cfg.merge_max_gap_leading * stats.leading && !(prev_number.is_some() && number.is_some()))
            {
                prev_lines.extend(lines);
                *prev_bbox = prev_bbox.union(&bbox);
                if prev_number.is_none() {
                    *prev_number = number;
                    *prev_number_line = number_line;
                } else if let Some(line) = number_line {
                    // Keep the glyphs of a second number drawable.
                    prev_lines.push(line);
                }
                continue;
            }
        }
        merged.push(Row::Equation {
            lines,
            bbox,
            number,
            number_line,
        });
    }
    merged
}

/// Index of the first row that belongs to the footnote area of the leaf
/// (`rows.len()` when there is none): below a footnote rule if there is
/// one, else a trailing run of small lines introduced by a footnote mark.
fn footnote_start(ctx: BlockContext, leaf: &Leaf, rows: &[Row], extent: &LeafExtent) -> usize {
    ruled_footnote_start(ctx, leaf, rows, extent)
        .or_else(|| marked_footnote_start(rows, ctx.stats, ctx.cfg))
        .unwrap_or(rows.len())
}

/// The first row below a footnote rule: a short, left-aligned horizontal
/// rule inside the leaf with only small text rows below it. With several
/// such rules, the one that starts the area earliest wins. `None` without
/// a footnote rule.
fn ruled_footnote_start(ctx: BlockContext, leaf: &Leaf, rows: &[Row], extent: &LeafExtent) -> Option<usize> {
    let (stats, cfg) = (ctx.stats, ctx.cfg);
    let text_rows: Vec<(usize, &Line)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| {
            if let Row::Text(line) = row {
                Some((i, line))
            } else {
                None
            }
        })
        .collect();
    let small = |line: &Line| line.size <= stats.body_size - cfg.footnote_size_drop_pt;
    let mut best: Option<usize> = None;
    for path in &ctx.page.paths {
        if !path.is_hrule() {
            continue;
        }
        let width = path.bbox.width();
        if width < cfg.footnote_rule_min_width_pt
            || width > cfg.footnote_rule_max_width_ratio * extent.width.max(cfg.footnote_rule_min_leaf_pt)
        {
            continue;
        }
        if (path.bbox.x0 - extent.left).abs() > cfg.footnote_rule_align_pt {
            continue;
        }
        if path.bbox.y0 < leaf.bbox.y0 || path.bbox.y0 > leaf.bbox.y1 {
            continue;
        }
        let below: Vec<&(usize, &Line)> = text_rows
            .iter()
            .filter(|(_, line)| line.bbox.y0 > path.bbox.y0 - cfg.footnote_rule_tol_pt)
            .collect();
        if below.is_empty() || !below.iter().all(|(_, line)| small(line)) {
            continue;
        }
        // Nothing but small text rows may follow.
        let first = below[0].0;
        if rows[first..].iter().any(|row| !matches!(row, Row::Text(_))) {
            continue;
        }
        if best.is_none_or(|start| first < start) {
            best = Some(first);
        }
    }
    best
}

/// Without a footnote rule: the first row of the trailing run of small text
/// rows that starts with a footnote mark and is set apart from the row
/// above. `None` when there is no such row (the leaf's first row never
/// starts a footnote area).
fn marked_footnote_start(rows: &[Row], stats: &DocStats, cfg: &BlocksConfig) -> Option<usize> {
    let mut k = rows.len();
    while k > 0 {
        match &rows[k - 1] {
            Row::Text(line) if line.size <= stats.body_size - cfg.footnote_run_size_drop_pt => k -= 1,
            _ => break,
        }
    }
    while k < rows.len() {
        let Row::Text(line) = &rows[k] else { break };
        let marked = footnote_marked(line);
        let gap_ok = k == 0
            || match &rows[k - 1] {
                Row::Text(prev) => {
                    line.baseline - prev.baseline
                        > cfg.footnote_gap_leading * stats.leading * line.size / stats.body_size
                }
                _ => true,
            };
        if marked && gap_ok && k > 0 {
            return Some(k);
        }
        k += 1;
    }
    None
}

/// Emits the blocks of a leaf's rows in order: floats and equations as they
/// are, consecutive text rows grouped (split at large gaps, size changes
/// and bold changes) and then cut into paragraphs. Rows from `fn_start` on
/// are footnotes.
fn emit_blocks(ctx: BlockContext, rows: Vec<Row>, fn_start: usize, loc: Loc, out: &mut Vec<RawBlock>) {
    let mut prev_baseline: Option<f32> = None;
    let mut group: Vec<(Line, f32)> = Vec::new(); // line, gap before
    let n_rows = rows.len();
    for (row_idx, row) in rows.into_iter().enumerate() {
        let footnote = row_idx >= fn_start;
        if row_idx == fn_start {
            flush_group(ctx, &mut group, loc, out, false);
            prev_baseline = None;
        }
        match row {
            Row::Float(idx) => {
                flush_group(ctx, &mut group, loc, out, footnote);
                out.push(RawBlock::Float { idx, loc });
                prev_baseline = None;
            }
            Row::Equation {
                lines,
                bbox,
                number,
                number_line,
            } => {
                flush_group(ctx, &mut group, loc, out, footnote);
                out.push(RawBlock::Equation(EquationBlock {
                    lines,
                    bbox,
                    number,
                    number_line,
                    loc,
                }));
                prev_baseline = None;
            }
            Row::Text(line) => {
                let gap = prev_baseline
                    .map(|baseline| line.baseline - baseline)
                    .unwrap_or(f32::INFINITY);
                if let Some((last, _)) = group.last()
                    && starts_new_group(ctx, last, &line, gap)
                {
                    flush_group(ctx, &mut group, loc, out, footnote);
                }
                prev_baseline = Some(line.baseline);
                group.push((line, gap));
            }
        }
        if row_idx + 1 == n_rows {
            flush_group(ctx, &mut group, loc, out, footnote);
        }
    }
    flush_group(ctx, &mut group, loc, out, false);
}

/// Whether text row `line` (at baseline distance `gap` from the previous
/// row) starts a new group after `last`: a large gap, a change of size, or
/// a change between bold and regular.
fn starts_new_group(ctx: BlockContext, last: &Line, line: &Line, gap: f32) -> bool {
    let (stats, cfg, fonts) = (ctx.stats, ctx.cfg, ctx.fonts);
    let expected = stats.leading * last.size.max(line.size) / stats.body_size;
    let ink_gap = line.ink.y0 - last.ink.y1;
    let big_gap =
        gap > cfg.block_gap_leading * expected + cfg.block_gap_slack_pt && ink_gap > cfg.block_ink_gap_em * line.size;
    let size_change = (last.size - line.size).abs() > cfg.block_size_change_pt;
    let bold_flip = bold_dominant(last, fonts, cfg) != bold_dominant(line, fonts, cfg)
        && last.char_count() > cfg.bold_flip_min_chars
        && line.char_count() > cfg.bold_flip_min_chars
        // A bold run-in heading that fills a whole line is rare; a flip
        // only separates blocks when the bold line stands alone.
        && (bold_dominant(last, fonts, cfg) || gap > cfg.bold_flip_gap_leading * expected);
    big_gap || size_change || bold_flip
}

/// Cuts the group of text rows into paragraphs and appends them to `out`
/// as text blocks (footnotes when `footnote`), leaving `group` empty. An
/// empty group adds nothing.
fn flush_group(ctx: BlockContext, group: &mut Vec<(Line, f32)>, loc: Loc, out: &mut Vec<RawBlock>, footnote: bool) {
    if group.is_empty() {
        return;
    }
    let cfg = ctx.cfg;
    let lines: Vec<(Line, f32)> = std::mem::take(group);
    let left = lines.iter().map(|line| line.0.bbox.x0).fold(f32::INFINITY, f32::min);
    let right = lines
        .iter()
        .map(|line| line.0.bbox.x1)
        .fold(f32::NEG_INFINITY, f32::max);
    let centers: Vec<f32> = lines.iter().map(|line| line.0.bbox.cx()).collect();
    let median_center = crate::common::geom::median(&centers, 0.0);
    let centered = lines.len() >= 2
        && centers
            .iter()
            .all(|center| (center - median_center).abs() <= cfg.centered_group_tol_pt)
        && lines
            .iter()
            .any(|line| (line.0.bbox.x0 - left).abs() > cfg.centered_group_min_offset_pt)
        && !footnote;

    for para in split_paragraphs(lines, right, centered, footnote, cfg) {
        let gap_before = para[0].1;
        let lines: Vec<Line> = para.into_iter().map(|line| line.0).collect();
        let bbox = lines.iter().fold(Rect::EMPTY, |acc, line| acc.union(&line.bbox));
        let block = TextBlock {
            lines,
            loc,
            left,
            right,
            centered,
            gap_before,
            bbox,
        };
        out.push(if footnote {
            RawBlock::Footnote(block)
        } else {
            RawBlock::Text(block)
        });
    }
}

/// Splits the lines of a group (with their gaps before) into paragraphs. A
/// centred group is one paragraph; otherwise a line starts a new paragraph
/// when one of the [`PARAGRAPH_RULES`] says so.
fn split_paragraphs(
    lines: Vec<(Line, f32)>,
    right: f32,
    centered: bool,
    footnote: bool,
    cfg: &BlocksConfig,
) -> Vec<Vec<(Line, f32)>> {
    let mut paras: Vec<Vec<(Line, f32)>> = Vec::new();
    for (line, gap) in lines {
        let Some(para) = paras.last_mut() else {
            paras.push(vec![(line, gap)]);
            continue;
        };
        let breaks = !centered && {
            let text = line.text();
            let candidate = ParagraphBreak {
                para,
                line: &line,
                text: &text,
                right,
                footnote,
            };
            PARAGRAPH_RULES.iter().any(|rule| rule(&candidate, cfg))
        };
        if breaks {
            paras.push(vec![(line, gap)]);
        } else {
            para.push((line, gap));
        }
    }
    paras
}

/// Rule: the line starts with a bullet, or (in footnotes) with a footnote
/// mark.
fn starts_with_marker(candidate: &ParagraphBreak, _cfg: &BlocksConfig) -> bool {
    (candidate.footnote && footnote_marked(candidate.line)) || BULLET.is_match(candidate.text)
}

/// Rule: the line starts with an enumeration label, aligned with a
/// paragraph that also starts with one: the next item of a list.
fn new_enumeration_item(candidate: &ParagraphBreak, cfg: &BlocksConfig) -> bool {
    let first = &candidate.para[0].0;
    NUM_LABEL.is_match(candidate.text)
        && NUM_LABEL.is_match(&first.text())
        && (candidate.line.bbox.x0 - first.bbox.x0).abs() <= cfg.para_label_align_em * candidate.line.size
}

/// Rule: the previous line ended early although the line's first word
/// would have fit (and it did not end with a hyphen).
fn follows_short_line(candidate: &ParagraphBreak, cfg: &BlocksConfig) -> bool {
    let prev = &candidate.para.last().unwrap().0;
    let em = candidate.line.size;
    !ends_with_hyphen(prev)
        && prev.bbox.x1 + cfg.para_word_space_em * em + candidate.line.first_word_width()
            < candidate.right - cfg.para_short_line_slack_pt
}

/// Rule: in a paragraph of two lines or more, the line starts left or right
/// of the paragraph's continuation lines.
fn indent_changes(candidate: &ParagraphBreak, cfg: &BlocksConfig) -> bool {
    if candidate.para.len() == 1 {
        return false;
    }
    let em = candidate.line.size;
    let rest_x0 = candidate.para[1..]
        .iter()
        .map(|line| line.0.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    candidate.line.bbox.x0 > rest_x0 + cfg.para_indent_change_em * em
        || candidate.line.bbox.x0 < rest_x0 - cfg.para_indent_change_em * em
}
