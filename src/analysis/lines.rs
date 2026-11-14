//! Glyphs -> text lines.
//!
//! Glyphs are clustered by baseline, split at large horizontal gaps, and
//! sub/superscripts are attached to the line they belong to.

use crate::backend::{Dir, FontInfo, Glyph, RawPage};
use crate::common::geom::{Rect, median, weighted_mode};
use crate::model::style::{BOLD, ITALIC, MATH, MONO, SANS, SUB, SUP};
use crate::model::{Line, LineGlyph, RotatedText};
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

/// A line of at most this many glyphs, all brackets or separators (")",
/// "})"), can close a code listing.
const FIXED_PITCH_BRACKETS_MAX: usize = 3;
/// Equal open gaps in a row that make a short spread word ("err"), enough
/// for a line next to a code listing to belong to it.
const FIXED_PITCH_SHORT_RUN: usize = 2;
/// Height (in points) of the bands of baselines in which copies of a glyph
/// are looked for; a copy is looked for in its band and the two next to it,
/// so offsets up to this height are found.
const DUPLICATE_BAND_PT: f32 = 8.0;
/// The combining long solidus overlay, which TeX fonts give the slash that
/// negates a relation (`\neq`, `\notin`).
const NEGATION: char = '\u{0338}';
/// Glyphs without a Unicode value all have the text U+FFFD, so two
/// different ones can sit close together: such a glyph is a copy of another
/// only when their origins are within this distance (in points).
const UNKNOWN_DUPLICATE_MAX_OFFSET_PT: f32 = 0.25;

/// The default thresholds, as a constant so that [`merge_row`] and
/// [`split_at_gaps`] (called from later stages that do not hold a
/// [`LinesConfig`]) build their lines with the same values as
/// [`build_lines`] does by default.
const DEFAULT_CONFIG: LinesConfig = LinesConfig {
    page_margin_pt: 5.0,
    near_black_max_value: 48,
    duplicate_max_offset_em: 0.1,
    rotated_max_angle_diff_deg: 2.0,
    rotated_max_offset_em: 0.25,
    rotated_max_backstep_em: 0.3,
    rotated_max_gap_em: 1.2,
    rotated_space_gap_em: 0.14,
    size_bin_pt: 0.25,
    default_size_pt: 10.0,
    baseline_size_tolerance_pt: 0.3,
    same_baseline_pt: 0.25,
    same_baseline_em: 0.03,
    merge_max_gap_em: 1.0,
    justified_max_gap_em: 2.5,
    justified_edge_tol_pt: 1.5,
    justified_max_indent_em: 3.0,
    justified_min_step_em: 0.7,
    justified_max_step_em: 2.0,
    justified_gutter_reach_em: 4.0,
    justified_gutter_tol_pt: 1.0,
    justified_max_gap_ratio: 3.0,
    gutter_zone: (0.25, 0.75),
    gutter_max_cover_ratio: 0.02,
    gutter_min_column_lines: 10,
    gutter_min_width_pt: 3.0,
    gutter_min_span_ratio: 0.7,
    gutter_window_rows: 6,
    gutter_edge_reach_pt: 4.0,
    gutter_window_min_lines: 4,
    gutter_window_min_width_pt: 5.0,
    gutter_window_space_factor: 2.0,
    gutter_window_min_piece_chars: 15,
    gutter_window_min_letter_ratio: 0.7,
    merge_max_size_factor: 2.2,
    math_axis_em: 0.25,
    math_reanchor_min_offset_em: 0.5,
    host_min_size_factor: 1.12,
    script_max_rise_em: 0.62,
    script_max_drop_em: 0.42,
    script_slack_em: 0.3,
    superscript_min_rise_em: 0.12,
    subscript_min_drop_em: 0.07,
    row_script_max_size_ratio: 0.9,
    space_gap_em: 0.14,
    tight_space_gap_em: 0.085,
    tight_space_min_count: 3,
    tight_space_ratio: 3.0,
    fixed_pitch_min_run: 3,
    fixed_pitch_gap_tolerance_pt: 0.05,
    fixed_pitch_min_gap_em: 0.03,
    fixed_pitch_max_gap_em: 0.4,
    fixed_pitch_space_gap_em: 0.45,
    fixed_pitch_min_lines: 3,
    fixed_pitch_max_line_gap_em: 4.0,
    space_lookback_count: 3,
    line_box_ascent_em: 0.75,
    line_box_descent_em: 0.25,
    accent_min_width_pt: 0.5,
    accent_min_overlap_ratio: 0.4,
};

/// Tuning parameters of line building. Part of [`LayoutConfig`](crate::config::LayoutConfig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinesConfig {
    /// Glyphs whose origin lies further than this (in points) outside the
    /// page are dropped: they are clipped away and never visible.
    pub page_margin_pt: f32,
    /// Colours whose three channels are all below this value are treated as
    /// black, so that text set in a very dark grey is not marked as coloured.
    pub near_black_max_value: u8,
    /// The same text drawn again with its origin at most this far (in em of
    /// its font size, horizontally and vertically) from an earlier copy is
    /// one glyph drawn several times (fake bold, shadows, the white halo
    /// that clears an underline around descenders) and is kept once. Two
    /// real glyphs of the same text are at least a narrow advance apart.
    pub duplicate_max_offset_em: f32,
    /// Largest difference (in degrees) between the angles of two rotated
    /// glyphs of the same run.
    pub rotated_max_angle_diff_deg: f32,
    /// Largest distance (in font sizes) of a rotated glyph from the baseline
    /// of the current run.
    pub rotated_max_offset_em: f32,
    /// How far (in font sizes) a rotated glyph may start before the previous
    /// glyph's origin and still continue the run (kerning, overlaps).
    pub rotated_max_backstep_em: f32,
    /// Largest gap (in font sizes, beyond the previous glyph's advance)
    /// between consecutive glyphs of a rotated run.
    pub rotated_max_gap_em: f32,
    /// A gap (in font sizes, beyond the previous glyph's advance) above
    /// which a space is inserted into a rotated run.
    pub rotated_space_gap_em: f32,
    /// Bin width (in points) of the size histogram whose mode is the size of
    /// a line.
    pub size_bin_pt: f32,
    /// Size (in points) of a line whose glyphs have no size histogram (never
    /// happens for non-empty lines; required by the histogram helper).
    pub default_size_pt: f32,
    /// Largest difference (in points) between a glyph's size and the line
    /// size for the glyph to vote for the line's baseline.
    pub baseline_size_tolerance_pt: f32,
    /// Fixed part (in points) of the largest baseline difference of segments
    /// that share a baseline.
    pub same_baseline_pt: f32,
    /// Size-relative part (in font sizes) of the largest baseline difference
    /// of segments that share a baseline.
    pub same_baseline_em: f32,
    /// Segments on one baseline are merged when the gap between them is at
    /// most this (in font sizes of the larger segment); wider gaps separate
    /// columns or table cells.
    pub merge_max_gap_em: f32,
    /// A justified line with few words can have a word gap wider than
    /// `merge_max_gap_em`. Such a gap is closed when it is at most this (in
    /// font sizes) and a neighbouring line shows the two pieces are the ends
    /// of one line (see [`merge_justified_gaps`]).
    pub justified_max_gap_em: f32,
    /// How closely (in points) the neighbouring line must end where the
    /// right piece ends, and start where the left piece starts.
    pub justified_edge_tol_pt: f32,
    /// The neighbouring line may start this much (in font sizes) left of the
    /// left piece: the left piece may be an indented first line.
    pub justified_max_indent_em: f32,
    /// Range of baseline distances (in font sizes) of the neighbouring line.
    pub justified_min_step_em: f32,
    /// See [`LinesConfig::justified_min_step_em`].
    pub justified_max_step_em: f32,
    /// A gap is a column gutter, not a stretched space, when another line
    /// within this many font sizes above or below has an edge at the same
    /// place: the lines of a column all end, and start, at one x.
    pub justified_gutter_reach_em: f32,
    /// Tolerance (in points) of that edge comparison.
    pub justified_gutter_tol_pt: f32,
    /// A justified line stretches all its word spaces: a gap between pieces
    /// is closed only when it is at most this many times the median word
    /// space inside the pieces (table cells are far apart, words are not).
    pub justified_max_gap_ratio: f32,
    /// A column gutter is looked for between these fractions of the page
    /// width ...
    pub gutter_zone: (f32, f32),
    /// ... as an x range that the glyphs of at most this fraction of the
    /// page's segments cover ...
    pub gutter_max_cover_ratio: f32,
    /// ... next to which, on both sides, the glyphs of at least this many
    /// segments stand (the two columns) ...
    pub gutter_min_column_lines: usize,
    /// ... and at least this wide (in points). See [`split_at_gutters`].
    pub gutter_min_width_pt: f32,
    /// A segment is cut at a gap between two glyphs that spans at least this
    /// fraction of the gutter.
    pub gutter_min_span_ratio: f32,
    /// Without a gutter over the whole page, one is looked for in windows
    /// of this many consecutive rows (a figure set into a column) ...
    pub gutter_window_rows: usize,
    /// The text beside a gutter is counted within this many points of its
    /// edge (column edges are ragged by a few points).
    pub gutter_edge_reach_pt: f32,
    /// ... with text from at least this many segments on both sides ...
    pub gutter_window_min_lines: usize,
    /// ... at least this wide (in points) ...
    pub gutter_window_min_width_pt: f32,
    /// ... and this many times the median word space of the window: word
    /// spaces lined up by chance in justified text are not a gutter.
    pub gutter_window_space_factor: f32,
    /// The pieces on each side of a window's gutter are running text: their
    /// median length is at least this many characters ...
    pub gutter_window_min_piece_chars: usize,
    /// ... and at least this share of their characters are letters.
    pub gutter_window_min_letter_ratio: f32,
    /// Segments on one baseline are not merged when one font is this many
    /// times larger than the other (a heading next to a margin note).
    pub merge_max_size_factor: f32,
    /// Height of the math axis above the baseline (in font sizes): the
    /// centre of radicals, big operators and tall delimiters.
    pub math_axis_em: f32,
    /// A segment whose ink centre is further than this (in font sizes) from
    /// its math axis hangs from its origin and is moved onto the baseline
    /// implied by its ink.
    pub math_reanchor_min_offset_em: f32,
    /// A host line must be at least this many times larger than a script
    /// attached to it.
    pub host_min_size_factor: f32,
    /// Highest a script's baseline may be above its host's (in host font
    /// sizes).
    pub script_max_rise_em: f32,
    /// Lowest a script's baseline may be below its host's (in host font
    /// sizes).
    pub script_max_drop_em: f32,
    /// How far (in host font sizes) a script may lie horizontally beyond its
    /// host's ends.
    pub script_slack_em: f32,
    /// A script raised by more than this (in host font sizes) is a
    /// superscript.
    pub superscript_min_rise_em: f32,
    /// A script lowered by more than this (in host font sizes) is a
    /// subscript; between the two it stays on the baseline.
    pub subscript_min_drop_em: f32,
    /// When fragments of one row are merged, a fragment smaller than this
    /// fraction of the main fragment's size can be a script.
    pub row_script_max_size_ratio: f32,
    /// A gap between glyphs wider than this (in font sizes) is a word space.
    /// About half the width of a typical interword space.
    pub space_gap_em: f32,
    /// A tightly set justified line shrinks its word spaces below
    /// [`space_gap_em`](Self::space_gap_em); a gap wider than this (in font
    /// sizes) still is a word space if it is also wider than
    /// [`tight_space_ratio`](Self::tight_space_ratio) times the usual gap
    /// between neighbouring glyphs of the line (letter-spaced text has
    /// wider gaps everywhere and is left alone).
    pub tight_space_gap_em: f32,
    /// Fewest tight word gaps a line needs before they count as spaces (see
    /// [`tight_space_gap_em`](Self::tight_space_gap_em)).
    pub tight_space_min_count: usize,
    /// How many times wider than the median gap between neighbouring glyphs
    /// of its line a gap must be to count as a tight word space.
    pub tight_space_ratio: f32,
    /// A code listing set in a proportional font on fixed columns (the
    /// LaTeX `listings` package) spreads the letters of each word evenly
    /// over its columns. A word with a run of this many or more equal gaps
    /// between its letters (see
    /// [`fixed_pitch_gap_tolerance_pt`](Self::fixed_pitch_gap_tolerance_pt))
    /// is spread.
    pub fixed_pitch_min_run: usize,
    /// Gaps between letters that differ by less than this (points) are equal.
    pub fixed_pitch_gap_tolerance_pt: f32,
    /// The equal gaps of a spread word are wider than this (in font sizes):
    /// the letters of ordinary text touch.
    pub fixed_pitch_min_gap_em: f32,
    /// A gap narrower than this (in font sizes) can be inside a spread word;
    /// wider, it separates words.
    pub fixed_pitch_max_gap_em: f32,
    /// In a line of spread words, a gap is a word space only when wider than
    /// this (in font sizes, about one column): narrower gaps are the spread
    /// letters and the space beside a narrow character.
    pub fixed_pitch_space_gap_em: f32,
    /// Fewest lines of spread words that make a listing; fewer are headings
    /// set letter-spaced.
    pub fixed_pitch_min_lines: usize,
    /// Lines of spread words this far apart or closer (baseline to baseline,
    /// in font sizes) belong to one listing, across a blank line and a line
    /// of brackets.
    pub fixed_pitch_max_line_gap_em: f32,
    /// How many preceding glyphs are considered for the right edge from which
    /// a gap is measured (overlapping accents and stacked scripts).
    pub space_lookback_count: usize,
    /// Top of a line's nominal box above the baseline (in font sizes).
    pub line_box_ascent_em: f32,
    /// Bottom of a line's nominal box below the baseline (in font sizes).
    pub line_box_descent_em: f32,
    /// Smallest width (in points) assumed for a glyph when measuring how much
    /// an accent overlaps a letter, so that zero-width glyphs do not divide
    /// by zero.
    pub accent_min_width_pt: f32,
    /// Smallest horizontal overlap (as a fraction of the narrower glyph) of
    /// an accent with a letter for the two to be composed.
    pub accent_min_overlap_ratio: f32,
}

impl Default for LinesConfig {
    fn default() -> Self {
        DEFAULT_CONFIG
    }
}

impl Line {
    /// Text of the line, with a single space wherever a glyph is marked as
    /// following a word gap.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for glyph in &self.glyphs {
            if glyph.space_before && !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&glyph.text);
        }
        text
    }

    /// Number of glyphs of the line (a glyph can hold several characters,
    /// e.g. ligatures).
    pub fn char_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Width of the first word (up to the first inter-word gap).
    pub fn first_word_width(&self) -> f32 {
        let mut end = self.bbox.x0;
        for (i, glyph) in self.glyphs.iter().enumerate() {
            if i > 0 && glyph.space_before {
                break;
            }
            end = end.max(glyph.x1);
        }
        end - self.bbox.x0
    }

    /// Style flags ([`crate::model::style`]) of `glyph`, a glyph of this
    /// line: the properties of its font plus its script level.
    ///
    /// Panics if the glyph's font is not in `fonts`.
    pub fn style_of(&self, glyph: &LineGlyph, fonts: &[FontInfo]) -> u8 {
        let font = &fonts[glyph.font as usize];
        let mut style = 0;
        if font.bold {
            style |= BOLD;
        }
        if font.italic {
            style |= ITALIC;
        }
        if font.mono || self.fixed_pitch {
            style |= MONO;
        }
        if font.math {
            style |= MATH;
        }
        if font.sans {
            style |= SANS;
        }
        match glyph.script {
            1 => style |= SUP,
            -1 => style |= SUB,
            _ => {}
        }
        style
    }

    /// Fraction of glyphs for which `pred` holds on the glyph's font.
    pub fn font_frac(&self, fonts: &[FontInfo], pred: impl Fn(&FontInfo) -> bool) -> f32 {
        if self.glyphs.is_empty() {
            return 0.0;
        }
        let n = self
            .glyphs
            .iter()
            .filter(|glyph| pred(&fonts[glyph.font as usize]))
            .count();
        n as f32 / self.glyphs.len() as f32
    }

    /// Whether the line begins with a superscript (a footnote or
    /// affiliation mark).
    pub fn starts_with_superscript(&self) -> bool {
        self.glyphs.first().is_some_and(|glyph| glyph.script == 1)
    }
}

/// A run of glyphs on a common baseline while lines are being built, with
/// its dominant size and baseline and its horizontal extent.
struct Segment {
    /// The glyphs, in no particular order until [`finalize`].
    glyphs: Vec<LineGlyph>,
    /// Baseline: median baseline of the glyphs of the dominant size.
    y: f32,
    /// Dominant font size of the glyphs on the baseline.
    size: f32,
    /// Left edge of the leftmost glyph.
    x0: f32,
    /// Right edge of the rightmost glyph.
    x1: f32,
}

impl Segment {
    /// A segment of the given (non-empty) glyphs with its measurements
    /// computed.
    fn from_glyphs(glyphs: Vec<LineGlyph>, cfg: &LinesConfig) -> Self {
        let mut segment = Segment {
            glyphs,
            y: 0.0,
            size: 0.0,
            x0: 0.0,
            x1: 0.0,
        };
        segment.refresh(cfg);
        segment
    }

    /// Recomputes extent, size and baseline after the glyphs changed.
    ///
    /// Panics if the segment has no glyphs.
    fn refresh(&mut self, cfg: &LinesConfig) {
        self.x0 = self.glyphs.iter().map(|glyph| glyph.x0).fold(f32::INFINITY, f32::min);
        self.x1 = self
            .glyphs
            .iter()
            .map(|glyph| glyph.x1)
            .fold(f32::NEG_INFINITY, f32::max);
        // Dominant size and baseline among the glyphs that sit on the baseline.
        let base: Vec<&LineGlyph> = self.glyphs.iter().filter(|glyph| glyph.script == 0).collect();
        let pool: Vec<&LineGlyph> = if base.is_empty() {
            self.glyphs.iter().collect()
        } else {
            base
        };
        self.size = weighted_mode(
            pool.iter().map(|glyph| (glyph.size, 1.0)),
            cfg.size_bin_pt,
            cfg.default_size_pt,
        );
        let size = self.size;
        let ys: Vec<f32> = pool
            .iter()
            .filter(|glyph| (glyph.size - size).abs() < cfg.baseline_size_tolerance_pt)
            .map(|glyph| glyph.y)
            .collect();
        self.y = if ys.is_empty() {
            pool[0].y
        } else {
            median(&ys, pool[0].y)
        };
    }
}

/// The rotated text runs of a page while its glyphs are read.
#[derive(Default)]
struct RotatedRuns {
    /// The runs found so far, in drawing order.
    runs: Vec<RotatedText>,
    /// Origin, advance and angle of the last rotated glyph.
    last: Option<(f32, f32, f32, f32)>,
}

impl RotatedRuns {
    /// Adds a rotated glyph: to the current run if it lies on the same
    /// baseline right after the previous glyph, otherwise as a new run.
    fn push(&mut self, glyph: &Glyph, cfg: &LinesConfig) {
        // The glyph continues the current run if it lies on the same
        // baseline, right after the previous glyph.
        let (dir_x, dir_y) = (glyph.angle.to_radians().cos(), glyph.angle.to_radians().sin());
        let step = self.last.map(|(x, y, adv, angle)| {
            let (dx, dy) = (glyph.x - x, glyph.y - y);
            let along = dx * dir_x + dy * dir_y;
            let across = (dx * dir_y - dy * dir_x).abs();
            (along, across, adv, angle)
        });
        let same = step.is_some_and(|(along, across, adv, angle)| {
            (angle - glyph.angle).abs() < cfg.rotated_max_angle_diff_deg
                && across < cfg.rotated_max_offset_em * glyph.size
                && along > -cfg.rotated_max_backstep_em * glyph.size
                && along < adv + cfg.rotated_max_gap_em * glyph.size
        });
        if same {
            let (along, _, adv, _) = step.unwrap();
            let run = self.runs.last_mut().unwrap();
            if along > adv + cfg.rotated_space_gap_em * glyph.size {
                run.text.push(' ');
            }
            run.text.push_str(&glyph.text);
            run.bbox = run.bbox.union(&glyph.ink);
            run.length = ((glyph.x - run.x).powi(2) + (glyph.y - run.y).powi(2)).sqrt() + glyph.adv.max(0.0);
            run.origins.push((glyph.x, glyph.y));
        } else {
            self.runs.push(RotatedText {
                text: glyph.text.clone(),
                bbox: glyph.ink,
                x: glyph.x,
                y: glyph.y,
                angle: glyph.angle,
                size: glyph.size,
                length: glyph.adv.max(0.0),
                font: glyph.font,
                color: display_color(glyph.color, cfg),
                origins: vec![(glyph.x, glyph.y)],
            });
        }
        self.last = Some((glyph.x, glyph.y, glyph.adv, glyph.angle));
    }
}

/// Merges fragments that belong to one visual row (e.g. a line interrupted by
/// an inline root or fraction) into a single line.
///
/// Fragments clearly smaller than the main (longest) fragment and raised or
/// lowered relative to it become super- or subscripts. Panics if `lines` is
/// empty.
pub fn merge_row(page: usize, lines: Vec<Line>, cfg: &LinesConfig) -> Line {
    if lines.len() == 1 {
        return lines.into_iter().next().unwrap();
    }
    let main = lines
        .iter()
        .max_by_key(|line| line.glyphs.len())
        .map(|line| (line.baseline, line.size))
        .unwrap();
    let fixed_pitch = lines.iter().any(|line| line.fixed_pitch);
    let mut glyphs = Vec::new();
    for line in lines {
        let dy = line.baseline - main.0;
        let small = line.size < cfg.row_script_max_size_ratio * main.1;
        let flag = if !small { 0 } else { script_level(dy, main.1, cfg) };
        for mut glyph in line.glyphs {
            if glyph.script == 0 {
                glyph.script = flag;
            }
            glyphs.push(glyph);
        }
    }
    let mut row = finalize(page, Segment::from_glyphs(glyphs, cfg), cfg);
    if fixed_pitch {
        set_fixed_pitch(&mut row, cfg);
    }
    row
}

/// Marks a line as set on fixed columns, with its word spaces measured
/// against the columns.
fn set_fixed_pitch(line: &mut Line, cfg: &LinesConfig) {
    line.fixed_pitch = true;
    let size = line.size;
    mark_spaces(&mut line.glyphs, size, true, cfg);
}

/// Splits a line at horizontal gaps of at least `min_gap` points. Used
/// inside tables, where neighbouring cells can be closer together than the
/// distance at which lines are normally broken.
///
/// Returns a copy of the line when there is no such gap.
pub fn split_at_gaps(line: &Line, min_gap: f32, cfg: &LinesConfig) -> Vec<Line> {
    let mut groups: Vec<Vec<LineGlyph>> = Vec::new();
    let mut right = f32::NEG_INFINITY;
    for glyph in &line.glyphs {
        if groups.is_empty() || glyph.x0 - right >= min_gap {
            groups.push(Vec::new());
        }
        groups.last_mut().unwrap().push(glyph.clone());
        right = right.max(glyph.x1);
    }
    if groups.len() <= 1 {
        return vec![line.clone()];
    }
    groups
        .into_iter()
        .map(|group| finalize(line.page, Segment::from_glyphs(group, cfg), cfg))
        .collect()
}

/// Splits a line at the x positions `cuts` (ascending), each of which lies in
/// a gap between two glyphs: the glyphs that start left of the first cut form
/// the first line, and so on. Used inside tables, where a column corridor
/// can run through the spaces of what was read as one line.
///
/// Returns a copy of the line when `cuts` is empty.
pub fn split_at_positions(line: &Line, cuts: &[f32], cfg: &LinesConfig) -> Vec<Line> {
    if cuts.is_empty() {
        return vec![line.clone()];
    }
    let mut groups: Vec<Vec<LineGlyph>> = vec![Vec::new(); cuts.len() + 1];
    for glyph in &line.glyphs {
        let group = cuts.iter().filter(|&&cut| glyph.x0 >= cut).count();
        groups[group].push(glyph.clone());
    }
    groups
        .into_iter()
        .filter(|group| group.iter().any(|glyph| !glyph.text.trim().is_empty()))
        .map(|group| finalize(line.page, Segment::from_glyphs(group, cfg), cfg))
        .collect()
}

/// Builds the text lines of a page from its glyphs.
///
/// Returns the horizontal lines, sorted by baseline then left edge, and the
/// runs of rotated text (which are not part of any line). Invisible glyphs
/// (OCR layers) are ignored when the page also has visible text.
pub fn build_lines(raw: &RawPage, cfg: &LinesConfig) -> (Vec<Line>, Vec<RotatedText>) {
    let (glyphs, rotated) = collect_glyphs(raw, cfg);

    // 1. Baseline groups, split at large gaps.
    let segments: Vec<Segment> = glyphs
        .into_iter()
        .map(|glyph| Segment::from_glyphs(vec![glyph], cfg))
        .collect();
    let mut segments = merge_same_baseline(segments, cfg);
    reanchor_hanging_math(&mut segments, cfg);

    // 2. Attach sub/superscripts to their host line, smallest first.
    let segments = attach_scripts(segments, cfg);

    // 3. Hosts may have grown towards each other.
    let segments = merge_same_baseline(segments, cfg);
    let segments = merge_justified_gaps(segments, cfg);
    let segments = split_at_gutters(segments, raw.width, cfg);

    let mut lines: Vec<Line> = segments
        .into_iter()
        .map(|segment| finalize(raw.index, segment, cfg))
        .collect();
    lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
    mark_fixed_pitch(&mut lines, cfg);
    (lines, rotated)
}

/// Removes the word spaces found after a letter of a math font that are
/// only its italic correction: TeX math sets a product of letters ("Var",
/// "xy") or a function and its argument ("f(x)") without space, but after
/// a slanted letter it adds the part of the letter's ink that overhangs its
/// advance, a gap as wide as a thin word space. The gap is measured again
/// from the ink of the letter; the space stays when it is still wider than
/// [`LinesConfig::space_gap_em`], and before a word of a text font.
pub fn join_math_letters(lines: &mut [Line], fonts: &[FontInfo], cfg: &LinesConfig) {
    let math = |glyph: &LineGlyph| fonts.get(glyph.font as usize).is_some_and(|font| font.math);
    let letter = |glyph: &LineGlyph| {
        let mut chars = glyph.text.chars();
        matches!((chars.next(), chars.next()), (Some(ch), None) if ch.is_alphabetic())
    };
    for line in lines {
        for i in 1..line.glyphs.len() {
            let (prev, cur) = (&line.glyphs[i - 1], &line.glyphs[i]);
            if !cur.space_before || !math(prev) || !letter(prev) || (letter(cur) && !math(cur)) {
                continue;
            }
            let gap = cur.x0 - prev.x1.max(prev.ink.x1);
            if gap <= cfg.space_gap_em * line.size {
                line.glyphs[i].space_before = false;
            }
        }
    }
}

/// Reads the glyphs of a page: drops hidden, blank, off-page and duplicate
/// glyphs, collects rotated glyphs into runs, and turns the remaining ones
/// into line glyphs (in drawing order).
fn collect_glyphs(raw: &RawPage, cfg: &LinesConfig) -> (Vec<LineGlyph>, Vec<RotatedText>) {
    let has_visible = raw.glyphs.iter().any(|glyph| !glyph.invisible);
    let mut copies = GlyphCopies::default();
    let mut glyphs: Vec<LineGlyph> = Vec::new();
    let mut rotated = RotatedRuns::default();

    for glyph in &raw.glyphs {
        if glyph.invisible && has_visible {
            continue;
        }
        if glyph.text.trim().is_empty() {
            continue;
        }
        if is_off_page(glyph, raw, cfg) {
            continue;
        }
        if glyph.dir == Dir::Rotated {
            rotated.push(glyph, cfg);
            continue;
        }
        if let Some(kept) = copies.find(glyph, cfg) {
            // The copy drawn last is the one on top, the one that is seen.
            if glyphs[kept].color == display_color(glyph.color, cfg) {
                glyphs[kept].extra.push((glyph.x, glyph.y));
            } else {
                let under = std::mem::replace(&mut glyphs[kept], line_glyph(glyph, cfg));
                glyphs[kept].extra = under.extra;
                glyphs[kept].extra.push((under.ox, under.oy));
            }
            continue;
        }
        copies.insert(glyph, glyphs.len());
        glyphs.push(line_glyph(glyph, cfg));
    }
    (glyphs, rotated.runs)
}

/// A glyph of the page as a glyph of a line (not yet raised, lowered or
/// spaced).
fn line_glyph(glyph: &Glyph, cfg: &LinesConfig) -> LineGlyph {
    LineGlyph {
        text: glyph.text.clone(),
        x0: glyph.x,
        x1: glyph.x + glyph.adv.max(0.0),
        y: glyph.y,
        size: glyph.size,
        font: glyph.font,
        script: 0,
        space_before: false,
        ink: glyph.ink,
        ox: glyph.x,
        oy: glyph.y,
        extra: Vec::new(),
        color: display_color(glyph.color, cfg),
    }
}

/// Whether the glyph's origin lies clearly outside the page.
fn is_off_page(glyph: &Glyph, raw: &RawPage, cfg: &LinesConfig) -> bool {
    glyph.x < -cfg.page_margin_pt
        || glyph.y < -cfg.page_margin_pt
        || glyph.x > raw.width + cfg.page_margin_pt
        || glyph.y > raw.height + cfg.page_margin_pt
}

/// A kept glyph: its index among the line glyphs and its origin.
type KeptGlyph = (usize, f32, f32);

/// The glyphs kept so far on a page, by text and band of baselines, to find
/// a glyph drawn again at (nearly) the same place.
#[derive(Default)]
struct GlyphCopies {
    /// (hash of the text, band of the baseline) -> the kept glyphs.
    by_band: HashMap<(u64, i32), Vec<KeptGlyph>>,
}

impl GlyphCopies {
    /// Index of a kept glyph of which `glyph` is a copy (see
    /// [`LinesConfig::duplicate_max_offset_em`]), if any.
    fn find(&self, glyph: &Glyph, cfg: &LinesConfig) -> Option<usize> {
        let tol = if glyph.text.contains('\u{FFFD}') {
            UNKNOWN_DUPLICATE_MAX_OFFSET_PT
        } else {
            cfg.duplicate_max_offset_em * glyph.size
        };
        let (hash, band) = Self::key(glyph);
        (band - 1..=band + 1)
            .filter_map(|band| self.by_band.get(&(hash, band)))
            .flatten()
            .find(|(_, x, y)| (glyph.x - x).abs() <= tol && (glyph.y - y).abs() <= tol)
            .map(|&(idx, _, _)| idx)
    }

    /// Records `glyph`, kept at index `idx`.
    fn insert(&mut self, glyph: &Glyph, idx: usize) {
        self.by_band
            .entry(Self::key(glyph))
            .or_default()
            .push((idx, glyph.x, glyph.y));
    }

    /// Hash of the text and band of the baseline of a glyph.
    fn key(glyph: &Glyph) -> (u64, i32) {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        glyph.text.hash(&mut hasher);
        (hasher.finish(), (glyph.y / DUPLICATE_BAND_PT).floor() as i32)
    }
}

/// The colour a glyph is shown in: near-black becomes black.
fn display_color(color: [u8; 3], cfg: &LinesConfig) -> [u8; 3] {
    if color.iter().all(|&channel| channel < cfg.near_black_max_value) {
        [0; 3]
    } else {
        color
    }
}

/// Script level of a fragment whose baseline is `dy` below the host's
/// (negative = above), for a host of font size `size`: 1 = superscript,
/// -1 = subscript, 0 = on the baseline.
fn script_level(dy: f32, size: f32, cfg: &LinesConfig) -> i8 {
    if dy < -cfg.superscript_min_rise_em * size {
        1
    } else if dy > cfg.subscript_min_drop_em * size {
        -1
    } else {
        0
    }
}

/// The combining character corresponding to a spacing accent (or the
/// combining character itself), or `None` when `ch` is not an accent.
fn combining_for(ch: char) -> Option<char> {
    Some(match ch {
        '\u{00B4}' | '\u{02CA}' => '\u{0301}',
        '`' | '\u{02CB}' => '\u{0300}',
        '\u{00A8}' => '\u{0308}',
        '\u{02C6}' | '^' => '\u{0302}',
        '\u{02DC}' | '~' => '\u{0303}',
        '\u{00AF}' | '\u{02C9}' => '\u{0304}',
        '\u{02C7}' => '\u{030C}',
        '\u{02D8}' => '\u{0306}',
        '\u{02D9}' => '\u{0307}',
        '\u{02DA}' => '\u{030A}',
        '\u{00B8}' => '\u{0327}',
        '\u{02DD}' => '\u{030B}',
        '\u{02DB}' => '\u{0328}',
        '\u{0300}'..='\u{036F}' => ch,
        _ => return None,
    })
}

/// TeX's OT1 encoding draws accents as separate glyphs overlapping the base
/// letter; fold them back into precomposed characters.
///
/// `glyphs` must be sorted left to right. An accent is composed with the
/// neighbouring single letter (next one preferred) it overlaps most, if the
/// overlap is large enough; the accent glyph is then removed.
fn compose_accents(glyphs: &mut Vec<LineGlyph>, cfg: &LinesConfig) {
    let mut i = 0;
    while i < glyphs.len() {
        let mut chars = glyphs[i].text.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            i += 1;
            continue;
        };
        let Some(mark) = combining_for(ch) else {
            i += 1;
            continue;
        };
        let target = if mark == NEGATION {
            negation_target(glyphs, i, cfg)
        } else {
            accent_target(glyphs, i, cfg)
        };
        let Some(target) = target else {
            i += 1;
            continue;
        };
        let base = match glyphs[target].text.as_str() {
            "\u{0131}" => "i".to_string(),
            "\u{0237}" => "j".to_string(),
            other => other.to_string(),
        };
        let composed: String = format!("{base}{mark}").nfc().collect();
        let (accent_x0, accent_x1) = (glyphs[i].x0, glyphs[i].x1);
        let ink = glyphs[i].ink;
        let accent_origin = (glyphs[i].ox, glyphs[i].oy);
        let accent_copies = std::mem::take(&mut glyphs[i].extra);
        let letter = &mut glyphs[target];
        letter.extra.push(accent_origin);
        letter.extra.extend(accent_copies);
        letter.text = composed;
        letter.x0 = letter.x0.min(accent_x0);
        letter.x1 = letter.x1.max(accent_x1);
        letter.ink = letter.ink.union(&ink);
        glyphs.remove(i);
        if target < i {
            // Removed an element after the target; stay at the same index.
            continue;
        }
    }
}

/// Index of the symbol the negation slash at `slash_idx` crosses out (TeX
/// draws `\neq` as a slash over `=`): the next or previous glyph, if it is
/// one character that has a precomposed negated form (`=` → `≠`, `∈` → `∉`)
/// and the inks of the two overlap enough. `None` when neither qualifies.
fn negation_target(glyphs: &[LineGlyph], slash_idx: usize, cfg: &LinesConfig) -> Option<usize> {
    let slash = &glyphs[slash_idx].ink;
    let overlap = |other: &LineGlyph| -> f32 {
        let mut chars = other.text.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return 0.0;
        };
        let composed: String = [ch, NEGATION].iter().collect::<String>().nfc().collect();
        if composed.chars().count() != 1 {
            return 0.0;
        }
        let ink = &other.ink;
        (slash.x1.min(ink.x1) - slash.x0.max(ink.x0)) / slash.width().min(ink.width()).max(cfg.accent_min_width_pt)
    };
    let next = glyphs.get(slash_idx + 1).map_or(0.0, overlap);
    let prev = slash_idx.checked_sub(1).map_or(0.0, |idx| overlap(&glyphs[idx]));
    if next >= cfg.accent_min_overlap_ratio && next >= prev {
        Some(slash_idx + 1)
    } else if prev >= cfg.accent_min_overlap_ratio {
        Some(slash_idx - 1)
    } else {
        None
    }
}

/// Index of the letter the accent at `accent_idx` belongs to: the next or
/// previous glyph, if it is a single letter at the same script level that the
/// accent overlaps enough. `None` when neither qualifies.
fn accent_target(glyphs: &[LineGlyph], accent_idx: usize, cfg: &LinesConfig) -> Option<usize> {
    let accent = &glyphs[accent_idx];
    let accent_width = (accent.x1 - accent.x0).max(cfg.accent_min_width_pt);
    let overlap = |other: &LineGlyph| -> f32 {
        let single_letter =
            other.text.chars().count() == 1 && other.text.chars().next().is_some_and(|ch| ch.is_alphabetic());
        if !single_letter || other.script != accent.script {
            return 0.0;
        }
        (accent.x1.min(other.x1) - accent.x0.max(other.x0))
            / accent_width.min((other.x1 - other.x0).max(cfg.accent_min_width_pt))
    };
    let next = if accent_idx + 1 < glyphs.len() {
        overlap(&glyphs[accent_idx + 1])
    } else {
        0.0
    };
    let prev = if accent_idx > 0 {
        overlap(&glyphs[accent_idx - 1])
    } else {
        0.0
    };
    if next >= cfg.accent_min_overlap_ratio && next >= prev {
        Some(accent_idx + 1)
    } else if prev >= cfg.accent_min_overlap_ratio {
        Some(accent_idx - 1)
    } else {
        None
    }
}

/// Turns a finished segment into a line: sorts its glyphs, composes
/// accents, marks word spaces, and picks the line's font (the most common
/// font of the glyphs on the baseline, lowest index on ties).
///
/// Panics if the segment has no glyphs.
fn finalize(page: usize, mut segment: Segment, cfg: &LinesConfig) -> Line {
    segment.glyphs.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    compose_accents(&mut segment.glyphs, cfg);
    segment.refresh(cfg);
    let size = segment.size;
    mark_spaces(&mut segment.glyphs, size, false, cfg);
    let ink = segment
        .glyphs
        .iter()
        .fold(Rect::EMPTY, |acc, glyph| acc.union(&glyph.ink));
    let font = {
        let mut counts: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
        for glyph in segment.glyphs.iter().filter(|glyph| glyph.script == 0) {
            *counts.entry(glyph.font).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by_key(|&(font, count)| (count, std::cmp::Reverse(font)))
            .map(|(font, _)| font)
            .unwrap_or(segment.glyphs[0].font)
    };
    Line {
        page,
        bbox: Rect::new(
            segment.x0,
            segment.y - cfg.line_box_ascent_em * size,
            segment.x1,
            segment.y + cfg.line_box_descent_em * size,
        ),
        ink,
        baseline: segment.y,
        size,
        font,
        glyphs: segment.glyphs,
        fixed_pitch: false,
    }
}

/// Finds the code listings of a page set on fixed columns in a
/// proportional font (see [`LinesConfig::fixed_pitch_min_run`]) and marks
/// their lines [`Line::fixed_pitch`], with word spaces measured against
/// the columns.
///
/// A listing is first found as at least [`LinesConfig::fixed_pitch_min_lines`]
/// lines with [`Spread::Columns`], each within
/// [`LinesConfig::fixed_pitch_max_line_gap_em`] of the next and overlapping
/// the others horizontally. It then takes in the lines between them and,
/// above and below, the neighbouring lines that have a spread word or only
/// brackets ("import (", ")"). A lone line of spread letters is a heading
/// set letter-spaced, and is left alone. `lines` are sorted by baseline.
fn mark_fixed_pitch(lines: &mut [Line], cfg: &LinesConfig) {
    let spreads: Vec<Spread> = lines
        .iter()
        .map(|line| spread_of(&line.glyphs, line.size, cfg))
        .collect();
    // Groups of lines with spread columns, each with the union of its boxes.
    let mut groups: Vec<(Vec<usize>, Rect)> = Vec::new();
    for idx in (0..lines.len()).filter(|&idx| spreads[idx] == Spread::Columns) {
        let line = &lines[idx];
        let joins = groups.last().is_some_and(|(group, hull)| {
            let prev = &lines[group[group.len() - 1]];
            line.baseline - prev.baseline <= cfg.fixed_pitch_max_line_gap_em * line.size && overlaps(&line.bbox, hull)
        });
        match groups.last_mut() {
            Some((group, hull)) if joins => {
                group.push(idx);
                *hull = hull.union(&line.bbox);
            }
            _ => groups.push((vec![idx], line.bbox)),
        }
    }
    let mut marked = vec![false; lines.len()];
    for (group, hull) in groups
        .iter()
        .filter(|(group, _)| group.len() >= cfg.fixed_pitch_min_lines)
    {
        let (first, last) = (group[0], group[group.len() - 1]);
        for idx in first..=last {
            marked[idx] |= overlaps(&lines[idx].bbox, hull);
        }
        // Extend over the neighbouring lines of the listing, up then down.
        let neighbour = |from: usize, to: usize| {
            (lines[to].baseline - lines[from].baseline).abs() <= cfg.fixed_pitch_max_line_gap_em * lines[to].size
        };
        let mut edge = first;
        for idx in (0..first).rev() {
            if !overlaps(&lines[idx].bbox, hull) {
                continue;
            }
            if spreads[idx] == Spread::None || !neighbour(edge, idx) {
                break;
            }
            marked[idx] = true;
            edge = idx;
        }
        let mut edge = last;
        for idx in last + 1..lines.len() {
            if !overlaps(&lines[idx].bbox, hull) {
                continue;
            }
            if spreads[idx] == Spread::None || !neighbour(edge, idx) {
                break;
            }
            marked[idx] = true;
            edge = idx;
        }
    }
    for (line, _) in lines.iter_mut().zip(&marked).filter(|(_, marked)| **marked) {
        set_fixed_pitch(line, cfg);
    }
}

/// Whether two boxes overlap horizontally.
fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1
}

/// How the letters of a line are spread (see [`spread_of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spread {
    /// No spread word: ordinary text.
    None,
    /// Spread words that all share one spacing, or only brackets: part of
    /// a listing when next to one.
    Some,
    /// Words spread with different spacings: set on fixed columns.
    Columns,
}

/// How the letters of a line (glyphs sorted left to right) are spread. The
/// LaTeX `listings` package sets code in a proportional font on fixed
/// columns, spreading each word evenly over its columns: a spread word
/// holds a run of [`LinesConfig::fixed_pitch_min_run`] or more equal, open
/// gaps between its letters. Words spread with different spacings are
/// [`Spread::Columns`]; one shared spacing is [`Spread::Some`] (it is also
/// letter-spaced text), as are a line with a short spread word only and a
/// line of brackets only.
fn spread_of(glyphs: &[LineGlyph], size: f32, cfg: &LinesConfig) -> Spread {
    let tolerance = cfg.fixed_pitch_gap_tolerance_pt;
    let mut trackings: Vec<f32> = Vec::new();
    // A short spread word ("err", "nil"): two equal open gaps.
    let mut short_word = false;
    // The current run of equal gaps: its gap and length.
    let mut run: Option<(f32, usize)> = None;
    let mut close_run = |run: &mut Option<(f32, usize)>| {
        if let Some((gap, len)) = run.take()
            && gap > cfg.fixed_pitch_min_gap_em * size
        {
            if len >= cfg.fixed_pitch_min_run {
                trackings.push(gap);
            } else if len >= FIXED_PITCH_SHORT_RUN {
                short_word = true;
            }
        }
    };
    for pair in glyphs.windows(2) {
        let gap = pair[1].x0 - pair[0].x1;
        if gap > cfg.fixed_pitch_max_gap_em * size {
            close_run(&mut run);
            continue;
        }
        match run {
            Some((first, len)) if (gap - first).abs() < tolerance => run = Some((first, len + 1)),
            _ => {
                close_run(&mut run);
                run = Some((gap, 1));
            }
        }
    }
    close_run(&mut run);
    let brackets = !glyphs.is_empty()
        && glyphs.len() <= FIXED_PITCH_BRACKETS_MAX
        && glyphs
            .iter()
            .all(|glyph| glyph.text.chars().all(|ch| "(){}[];,".contains(ch)));
    match trackings.first() {
        Some(&first) if trackings.iter().any(|tracking| (tracking - first).abs() >= tolerance) => Spread::Columns,
        Some(_) => Spread::Some,
        None if brackets || short_word => Spread::Some,
        None => Spread::None,
    }
}

/// Sets `space_before` on every glyph (sorted left to right) that follows a
/// gap wider than a word space. Between two scripts the gap is measured
/// against the larger script size, otherwise against the line size `size`.
///
/// A line set tightly (justified text with shrunk spaces) has word gaps
/// between [`LinesConfig::tight_space_gap_em`] and
/// [`LinesConfig::space_gap_em`]. They count as spaces when the line has at
/// least [`LinesConfig::tight_space_min_count`] of them and each is well
/// above the usual gap between the letters of the line; a single such gap
/// in a line of normal spaces is kerning (math italics, "WH").
fn mark_spaces(glyphs: &mut [LineGlyph], size: f32, fixed_pitch: bool, cfg: &LinesConfig) {
    let n = glyphs.len();
    let usual_gap = median_glyph_gap(glyphs).max(0.0);
    // The gap before each glyph (index 0 has none) and the font size it is
    // measured against.
    let mut gaps: Vec<(f32, f32)> = vec![(f32::NEG_INFINITY, size); n];
    for i in 1..n {
        let (prev, cur) = (&glyphs[i - 1], &glyphs[i]);
        let reference = if prev.script != 0 && cur.script != 0 {
            prev.size.max(cur.size)
        } else {
            size
        };
        // Use the furthest right edge seen so far so that overlapping glyphs
        // (accents, stacked scripts) do not create spurious gaps.
        let right = glyphs[..i]
            .iter()
            .rev()
            .take(cfg.space_lookback_count)
            .map(|glyph| glyph.x1)
            .fold(f32::NEG_INFINITY, f32::max);
        gaps[i] = (cur.x0 - right.max(prev.x1), reference);
    }
    let is_tight = |&(gap, reference): &(f32, f32)| {
        gap > cfg.tight_space_gap_em * reference
            && gap <= cfg.space_gap_em * reference
            && gap > cfg.tight_space_ratio * usual_gap
    };
    // A tight word space lies between two letters of the same script, and
    // its neighbours are no gaps: runs of gaps are letter-spaced text or
    // monospaced code.
    let tight: Vec<bool> = (0..n)
        .map(|i| {
            i > 0
                && is_tight(&gaps[i])
                && glyphs[i].script == 0
                && glyphs[i - 1].script == 0
                && !(i > 1 && is_tight(&gaps[i - 1]))
                && !(i + 1 < n && is_tight(&gaps[i + 1]))
        })
        .collect();
    let tight_spaces = tight.iter().filter(|&&is_space| is_space).count() >= cfg.tight_space_min_count;
    for (i, glyph) in glyphs.iter_mut().enumerate() {
        glyph.space_before = if fixed_pitch {
            gaps[i].0 > cfg.fixed_pitch_space_gap_em * gaps[i].1
        } else {
            gaps[i].0 > cfg.space_gap_em * gaps[i].1 || (tight_spaces && tight[i])
        };
    }
    if let Some(glyph) = glyphs.first_mut() {
        glyph.space_before = false;
    }
}

/// The median gap between neighbouring glyphs of a line (in points; negative
/// when they overlap), the usual spacing of its letters. Gaps are measured
/// between the boxes of consecutive glyphs in the order given. 0 for a line
/// of fewer than two glyphs.
fn median_glyph_gap(glyphs: &[LineGlyph]) -> f32 {
    let mut gaps: Vec<f32> = glyphs.windows(2).map(|pair| pair[1].x0 - pair[0].x1).collect();
    gaps.sort_by(f32::total_cmp);
    gaps.get(gaps.len() / 2).copied().unwrap_or(0.0)
}

/// Merges segments that share a baseline and are separated by less than
/// [`LinesConfig::merge_max_gap_em`] (in units of the larger font size).
fn merge_same_baseline(mut segments: Vec<Segment>, cfg: &LinesConfig) -> Vec<Segment> {
    segments.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let mut out: Vec<Segment> = Vec::with_capacity(segments.len());
    let mut rest = segments.into_iter().peekable();
    while let Some(first) = rest.next() {
        // Collect the group of segments with (nearly) the same baseline as its first one.
        let (first_y, reach) = (first.y, cfg.same_baseline_pt + cfg.same_baseline_em * first.size);
        let mut group = vec![first];
        while let Some(segment) = rest.next_if(|segment| segment.y - first_y <= reach) {
            group.push(segment);
        }
        group.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        merge_group(group, cfg, &mut out);
    }
    out
}

/// Merges the segments of one baseline group, sorted from left to right, and
/// appends the results to `out`.
///
/// While a segment grows, only what the merge test reads is kept up to date
/// (its extent, and its dominant size through a [`SizeTally`]); its glyphs
/// are measured in full once, when it is complete. The result is the same as
/// measuring it after every merge, without rescanning a long line once per
/// glyph.
fn merge_group(group: Vec<Segment>, cfg: &LinesConfig, out: &mut Vec<Segment>) {
    let mut cur: Option<(Segment, Option<SizeTally>)> = None;
    for segment in group {
        match cur.as_mut() {
            Some((current, tally))
                if segment.x0 - current.x1 <= cfg.merge_max_gap_em * current.size.max(segment.size)
                    && (current.size / segment.size).max(segment.size / current.size) < cfg.merge_max_size_factor =>
            {
                let tally = tally.get_or_insert_with(|| SizeTally::new(&current.glyphs, cfg.size_bin_pt));
                tally.add(&segment.glyphs, cfg.size_bin_pt);
                current.x0 = current.x0.min(segment.x0);
                current.x1 = current.x1.max(segment.x1);
                current.glyphs.extend(segment.glyphs);
                current.size = tally.dominant(cfg.size_bin_pt, cfg.default_size_pt);
            }
            _ => {
                out.extend(
                    cur.take()
                        .map(|(current, tally)| finish_merge(current, tally.is_some(), cfg)),
                );
                cur = Some((segment, None));
            }
        }
    }
    out.extend(cur.map(|(current, tally)| finish_merge(current, tally.is_some(), cfg)));
}

/// A segment out of [`merge_group`]: measured in full if it took in others.
fn finish_merge(mut segment: Segment, merged: bool, cfg: &LinesConfig) -> Segment {
    if merged {
        segment.refresh(cfg);
    }
    segment
}

/// Glyph counts per font-size bucket of a growing segment, from which its
/// dominant size is read as [`Segment::refresh`] computes it: the most common
/// bucket among the glyphs on the baseline (all glyphs if none is), the
/// smaller bucket on a tie.
struct SizeTally {
    /// Glyphs on the baseline (not sub- or superscripts), per size bucket.
    base: HashMap<i32, u32>,
    /// All glyphs, per size bucket.
    all: HashMap<i32, u32>,
}

impl SizeTally {
    /// The counts of `glyphs`, in buckets of `step` points.
    fn new(glyphs: &[LineGlyph], step: f32) -> Self {
        let mut tally = SizeTally {
            base: HashMap::new(),
            all: HashMap::new(),
        };
        tally.add(glyphs, step);
        tally
    }

    /// Counts `glyphs` in.
    fn add(&mut self, glyphs: &[LineGlyph], step: f32) {
        for glyph in glyphs {
            // The same rounding as `weighted_mode`.
            let bucket = (glyph.size / step).round() as i32;
            *self.all.entry(bucket).or_insert(0) += 1;
            if glyph.script == 0 {
                *self.base.entry(bucket).or_insert(0) += 1;
            }
        }
    }

    /// The dominant size, or `default` with no glyphs.
    fn dominant(&self, step: f32, default: f32) -> f32 {
        let hist = if self.base.is_empty() { &self.all } else { &self.base };
        hist.iter()
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map_or(default, |(bucket, _)| *bucket as f32 * step)
    }
}

/// Joins the pieces of a justified line whose word gaps were stretched
/// beyond [`LinesConfig::merge_max_gap_em`]: the first line of a caption set
/// next to a figure ("Figure 3:   Transfer to ImageNet.     While") has few
/// words to share the space.
///
/// A run of pieces on one baseline is joined only when its word spaces are
/// stretched like its gaps, its pieces hold words, a line directly above or
/// below runs from the first piece's start (or an indent before it) to the
/// last piece's end without a break, and no line nearby has an edge at
/// either side of a gap: the lines of two columns all end, and start, at the
/// gutter.
fn merge_justified_gaps(mut segments: Vec<Segment>, cfg: &LinesConfig) -> Vec<Segment> {
    segments.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < segments.len() {
        // The longest run from i whose gaps could be stretched spaces.
        let mut end = i;
        while end + 1 < segments.len() && joinable(&segments[end], &segments[end + 1], cfg) {
            end += 1;
        }
        // Its longest prefix that is one justified line.
        let found = (i + 1..=end).rev().find(|&last| {
            let pieces = &segments[i..=last];
            let widest = pieces
                .windows(2)
                .map(|pair| pair[1].x0 - pair[0].x1)
                .fold(0.0, f32::max);
            stretched(pieces, widest, cfg)
                && pieces[1..].iter().all(has_word)
                && spanned(pieces, &segments, cfg)
                && pieces
                    .windows(2)
                    .all(|pair| !gutter(&pair[0], &pair[1], &segments, cfg))
        });
        match found {
            Some(last) => {
                runs.push((i, last));
                i = last + 1;
            }
            None => i += 1,
        }
    }
    for &(first, last) in runs.iter().rev() {
        let rest: Vec<Segment> = segments.drain(first + 1..=last).collect();
        for piece in rest {
            segments[first].glyphs.extend(piece.glyphs);
        }
        segments[first].refresh(cfg);
    }
    segments
}

/// Whether `right` follows `left` on the same baseline, at a gap that a
/// stretched word space may reach.
fn joinable(left: &Segment, right: &Segment, cfg: &LinesConfig) -> bool {
    let gap = right.x0 - left.x1;
    (right.y - left.y).abs() <= cfg.same_baseline_pt + cfg.same_baseline_em * left.size
        && gap > 0.0
        && gap <= cfg.justified_max_gap_em * left.size.max(right.size)
        && (left.size / right.size).max(right.size / left.size) < cfg.merge_max_size_factor
}

/// Whether the word spaces inside `pieces` are stretched like the widest
/// gap between them: a justified line stretches all its spaces, while table
/// cells are far apart and the words inside them are not.
fn stretched(pieces: &[Segment], widest: f32, cfg: &LinesConfig) -> bool {
    let mut spaces: Vec<f32> = Vec::new();
    for piece in pieces {
        let mut glyphs: Vec<&LineGlyph> = piece
            .glyphs
            .iter()
            .filter(|glyph| !glyph.text.trim().is_empty())
            .collect();
        glyphs.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        spaces.extend(
            glyphs
                .windows(2)
                .map(|pair| pair[1].x0 - pair[0].x1)
                .filter(|&gap| gap > cfg.space_gap_em * piece.size),
        );
    }
    if spaces.len() < 2 {
        return false;
    }
    spaces.sort_by(f32::total_cmp);
    widest <= cfg.justified_max_gap_ratio * spaces[spaces.len() / 2]
}

/// Whether `piece` holds a word of two lower-case letters: text, not a
/// table cell of numbers.
fn has_word(piece: &Segment) -> bool {
    let mut glyphs: Vec<&LineGlyph> = piece.glyphs.iter().collect();
    glyphs.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let lower = |glyph: &LineGlyph| !glyph.text.is_empty() && glyph.text.chars().all(char::is_lowercase);
    glyphs.windows(2).any(|pair| lower(pair[0]) && lower(pair[1]))
}

/// Whether a line directly above or below `pieces` runs from the first
/// piece's start (or an indent before it) to the last piece's end.
fn spanned(pieces: &[Segment], all: &[Segment], cfg: &LinesConfig) -> bool {
    let (first, last) = (&pieces[0], &pieces[pieces.len() - 1]);
    let size = pieces.iter().map(|piece| piece.size).fold(0.0, f32::max);
    all.iter().any(|other| {
        let step = (other.y - first.y).abs();
        step >= cfg.justified_min_step_em * size
            && step <= cfg.justified_max_step_em * size
            && (other.size - size).abs() <= cfg.justified_edge_tol_pt
            && other.x0 <= first.x0 + cfg.justified_edge_tol_pt
            && other.x0 >= first.x0 - cfg.justified_max_indent_em * size
            && (other.x1 - last.x1).abs() <= cfg.justified_edge_tol_pt
    })
}

/// Whether another line near the gap between `left` and `right` has an edge
/// at either side of it, as the lines of two columns do at their gutter.
fn gutter(left: &Segment, right: &Segment, all: &[Segment], cfg: &LinesConfig) -> bool {
    let size = left.size.max(right.size);
    all.iter().any(|other| {
        !std::ptr::eq(other, left)
            && !std::ptr::eq(other, right)
            && (other.y - left.y).abs() <= cfg.justified_gutter_reach_em * size
            && ((other.x1 - left.x1).abs() <= cfg.justified_gutter_tol_pt
                || (other.x0 - right.x0).abs() <= cfg.justified_gutter_tol_pt)
    })
}

/// Splits segments at a column gutter they run across. Two columns set
/// with a gutter about one word space wide (the Batch Normalization paper),
/// or a figure set into a column with its caption beside the text, are
/// joined line by line into one segment. Their gutter shows as an x range
/// that hardly any glyph covers, with text on both sides, over the whole
/// page or over a run of consecutive lines (see [`find_gutter`]). A segment
/// is cut where its gap spans that range; pages without one are left alone.
fn split_at_gutters(segments: Vec<Segment>, page_width: f32, cfg: &LinesConfig) -> Vec<Segment> {
    let zone = (cfg.gutter_zone.0 * page_width, cfg.gutter_zone.1 * page_width);
    let all: Vec<&Segment> = segments.iter().collect();
    // One gutter per segment: the page's, or that of the window of lines it is in.
    let mut gutters: Vec<Option<(f32, f32)>> = vec![None; segments.len()];
    if let Some(gutter) = find_gutter(&all, zone, cfg.gutter_min_column_lines, cfg.gutter_min_width_pt, cfg) {
        gutters.fill(Some(gutter));
    } else {
        let mut order: Vec<usize> = (0..segments.len()).collect();
        order.sort_by(|&a, &b| segments[a].y.total_cmp(&segments[b].y));
        // Rows of segments on one baseline.
        let mut rows: Vec<Vec<usize>> = Vec::new();
        for index in order {
            match rows.last_mut() {
                Some(row)
                    if (segments[row[0]].y - segments[index].y).abs()
                        <= cfg.same_baseline_pt + cfg.same_baseline_em * segments[index].size =>
                {
                    row.push(index)
                }
                _ => rows.push(vec![index]),
            }
        }
        let rows_per_window = cfg.gutter_window_rows;
        for start in (0..rows.len()).step_by((rows_per_window / 2).max(1)) {
            let members: Vec<usize> = rows[start..(start + rows_per_window).min(rows.len())].concat();
            let window: Vec<&Segment> = members.iter().map(|&index| &segments[index]).collect();
            let min_width = cfg
                .gutter_window_min_width_pt
                .max(cfg.gutter_window_space_factor * median_space(&window, cfg));
            let lines = cfg.gutter_window_min_lines;
            if let Some(gutter) = find_gutter(&window, zone, lines, min_width, cfg)
                .filter(|&gutter| prose_on_both_sides(&window, gutter, cfg))
            {
                for index in members {
                    gutters[index] = gutters[index].or(Some(gutter));
                }
            }
        }
    }
    let mut out = Vec::with_capacity(segments.len());
    for (segment, gutter) in segments.into_iter().zip(gutters) {
        match gutter {
            Some(gutter) => out.extend(cut_at_gutter(segment, gutter, cfg)),
            None => out.push(segment),
        }
    }
    out
}

/// Whether the pieces on both sides of `gutter` in a window are running
/// text: long enough and mostly letters. A display equation and the column
/// of its conditions ("ℓ = 1 . . . L") have a gap like a gutter, but are
/// not two columns of text.
fn prose_on_both_sides(segments: &[&Segment], gutter: (f32, f32), cfg: &LinesConfig) -> bool {
    let side_is_prose = |left: bool| {
        let pieces: Vec<String> = segments
            .iter()
            .map(|segment| {
                visible_glyphs(segment)
                    .into_iter()
                    .filter(|glyph| {
                        if left {
                            glyph.x1 <= gutter.1
                        } else {
                            glyph.x0 >= gutter.0
                        }
                    })
                    .map(|glyph| glyph.text.as_str())
                    .collect::<String>()
            })
            .filter(|piece| !piece.is_empty())
            .collect();
        let mut lengths: Vec<usize> = pieces.iter().map(|piece| piece.chars().count()).collect();
        lengths.sort_unstable();
        let chars: usize = lengths.iter().sum();
        let letters: usize = pieces
            .iter()
            .map(|piece| piece.chars().filter(|ch| ch.is_alphabetic()).count())
            .sum();
        lengths
            .get(lengths.len() / 2)
            .is_some_and(|&median| median >= cfg.gutter_window_min_piece_chars)
            && letters as f32 >= cfg.gutter_window_min_letter_ratio * chars as f32
    };
    side_is_prose(true) && side_is_prose(false)
}

/// The visible glyphs of `segment`, left to right.
fn visible_glyphs(segment: &Segment) -> Vec<&LineGlyph> {
    let mut glyphs: Vec<&LineGlyph> = segment
        .glyphs
        .iter()
        .filter(|glyph| !glyph.text.trim().is_empty())
        .collect();
    glyphs.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    glyphs
}

/// The median word space of `segments` (0 when there is none).
fn median_space(segments: &[&Segment], cfg: &LinesConfig) -> f32 {
    let mut spaces: Vec<f32> = segments
        .iter()
        .flat_map(|segment| {
            let glyphs = visible_glyphs(segment);
            glyphs
                .windows(2)
                .map(|pair| pair[1].x0 - pair[0].x1)
                .filter(|&gap| gap > cfg.space_gap_em * segment.size)
                .collect::<Vec<f32>>()
        })
        .collect();
    spaces.sort_by(f32::total_cmp);
    spaces.get(spaces.len() / 2).copied().unwrap_or(0.0)
}

/// The gutter of `segments` within the x range `zone`: the widest run of
/// 1 pt bins that the glyphs of hardly any segment cover, at least
/// `min_width` wide, with the glyphs of at least `min_lines` segments right
/// next to it on both sides, and with some segment running across it.
fn find_gutter(
    segments: &[&Segment],
    zone: (f32, f32),
    min_lines: usize,
    min_width: f32,
    cfg: &LinesConfig,
) -> Option<(f32, f32)> {
    let (lo, hi) = zone;
    let bins = (hi - lo).max(0.0).ceil() as usize;
    if bins == 0 || segments.len() < min_lines {
        return None;
    }
    let mut covered = vec![0usize; bins];
    let mut gapped = vec![0usize; bins];
    let bin_of = |x: f32| ((x - lo).floor().max(0.0) as usize).min(bins);
    for segment in segments {
        let glyphs = visible_glyphs(segment);
        let mut ink = vec![false; bins];
        for glyph in &glyphs {
            let (from, to) = (bin_of(glyph.x0), bin_of(glyph.x1));
            for flag in ink.iter_mut().take(to).skip(from) {
                *flag = true;
            }
        }
        for (count, flag) in covered.iter_mut().zip(&ink) {
            *count += usize::from(*flag);
        }
        for pair in glyphs.windows(2) {
            let (from, to) = (bin_of(pair[0].x1), bin_of(pair[1].x0));
            for count in gapped.iter_mut().take(to).skip(from) {
                *count += 1;
            }
        }
    }
    let max_cover = (cfg.gutter_max_cover_ratio * segments.len() as f32).max(1.0) as usize;
    let mut best: Option<(usize, usize)> = None;
    let mut start = None;
    for bin in 0..=bins {
        match (bin < bins && covered[bin] <= max_cover, start) {
            (true, None) => start = Some(bin),
            (false, Some(first)) => {
                // Ragged column edges: the most lines within a few points.
                let reach = cfg.gutter_edge_reach_pt as usize;
                let left = covered[first.saturating_sub(reach)..first]
                    .iter()
                    .max()
                    .copied()
                    .unwrap_or(0);
                let right = covered[bin..(bin + reach).min(bins)].iter().max().copied().unwrap_or(0);
                let flanked = left >= min_lines && right >= min_lines;
                let crossed = gapped[first..bin].iter().any(|&count| count > 0);
                if flanked && crossed && best.is_none_or(|(a, b)| bin - first > b - a) {
                    best = Some((first, bin));
                }
                start = None;
            }
            _ => {}
        }
    }
    best.filter(|(first, last)| (last - first) as f32 >= min_width)
        .map(|(first, last)| (lo + first as f32, lo + last as f32))
}

/// Cuts `segment` where the gap between two of its glyphs spans (most of)
/// the `gutter`.
fn cut_at_gutter(segment: Segment, gutter: (f32, f32), cfg: &LinesConfig) -> Vec<Segment> {
    let (gutter_x0, gutter_x1) = gutter;
    let mut glyphs = segment.glyphs.clone();
    glyphs.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let cut = glyphs.windows(2).position(|pair| {
        pair[1].x0.min(gutter_x1) - pair[0].x1.max(gutter_x0) >= cfg.gutter_min_span_ratio * (gutter_x1 - gutter_x0)
    });
    match cut {
        Some(at) => {
            let right = glyphs.split_off(at + 1);
            vec![Segment::from_glyphs(glyphs, cfg), Segment::from_glyphs(right, cfg)]
        }
        None => vec![segment],
    }
}

/// Glyphs of math extension fonts (radicals, big operators, tall
/// delimiters) hang from their origin instead of sitting on the baseline.
/// Re-anchors such segments on the baseline implied by their ink, which is
/// centred on the math axis.
fn reanchor_hanging_math(segments: &mut [Segment], cfg: &LinesConfig) {
    for segment in segments.iter_mut() {
        let ink = segment
            .glyphs
            .iter()
            .fold(Rect::EMPTY, |acc, glyph| acc.union(&glyph.ink));
        if ink.is_unset() {
            continue;
        }
        let axis = segment.y - cfg.math_axis_em * segment.size;
        if (ink.cy() - axis).abs() > cfg.math_reanchor_min_offset_em * segment.size {
            let y = ink.cy() + cfg.math_axis_em * segment.size;
            for glyph in segment.glyphs.iter_mut() {
                glyph.y = y;
            }
            segment.refresh(cfg);
        }
    }
}

/// Attaches sub/superscripts to their host segment, smallest segments first,
/// marking their glyphs with the script level. Returns the remaining
/// segments in their original order.
fn attach_scripts(mut segments: Vec<Segment>, cfg: &LinesConfig) -> Vec<Segment> {
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by(|&a, &b| segments[a].size.total_cmp(&segments[b].size));
    let mut slots: Vec<Option<Segment>> = segments.drain(..).map(Some).collect();
    for script_idx in order {
        let Some(host_idx) = find_host(&slots, script_idx, cfg) else {
            continue;
        };
        let script = slots[script_idx].take().unwrap();
        let host = slots[host_idx].as_mut().unwrap();
        let dy = script.y - host.y;
        let flag = script_level(dy, host.size, cfg);
        for mut glyph in script.glyphs {
            // A script of a script stays inside its parent's level.
            if flag != 0 {
                glyph.script = flag;
            }
            host.glyphs.push(glyph);
        }
        host.x0 = host.x0.min(script.x0);
        host.x1 = host.x1.max(script.x1);
    }
    slots.into_iter().flatten().collect()
}

/// The segment that the segment in `slots[script_idx]` is a script of: a
/// clearly larger segment whose baseline is close and that it touches
/// horizontally, the one with the closest baseline (relative to its size)
/// winning. `None` when the slot is empty or there is no such host.
fn find_host(slots: &[Option<Segment>], script_idx: usize, cfg: &LinesConfig) -> Option<usize> {
    let script = slots[script_idx].as_ref()?;
    let (script_y, script_size, script_x0, script_x1) = (script.y, script.size, script.x0, script.x1);
    let mut best: Option<(usize, f32)> = None;
    for (host_idx, host) in slots.iter().enumerate() {
        let Some(host) = host else { continue };
        if host_idx == script_idx || host.size < cfg.host_min_size_factor * script_size {
            continue;
        }
        let dy = script_y - host.y;
        if dy < -cfg.script_max_rise_em * host.size || dy > cfg.script_max_drop_em * host.size {
            continue;
        }
        let slack = cfg.script_slack_em * host.size;
        if script_x0 > host.x1 + slack || script_x1 < host.x0 - slack {
            continue;
        }
        let score = dy.abs() / host.size;
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((host_idx, score));
        }
    }
    best.map(|(host_idx, _)| host_idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A horizontal glyph of 10 pt type at (`x`, 100) in `color`.
    fn glyph(text: &str, x: f32, color: [u8; 3]) -> Glyph {
        Glyph {
            text: text.to_string(),
            x,
            y: 100.0,
            adv: 5.0,
            size: 10.0,
            font: 0,
            dir: Dir::Horizontal,
            angle: 0.0,
            invisible: false,
            color,
            ink: Rect::new(x, 93.0, x + 5.0, 100.0),
        }
    }

    /// A page of 200 × 200 pt with these glyphs.
    fn page(glyphs: Vec<Glyph>) -> RawPage {
        RawPage {
            width: 200.0,
            height: 200.0,
            glyphs,
            ..Default::default()
        }
    }

    #[test]
    fn white_halo_copies_give_way_to_the_text_on_top() {
        // An underlined "g": white copies shifted by half a point clear the
        // rule around the descender, then the black letter is drawn on top.
        let white = [255; 3];
        let raw = page(vec![
            glyph("g", 49.5, white),
            glyph("g", 50.5, white),
            glyph("g", 50.0, [0; 3]),
        ]);
        let (glyphs, _) = collect_glyphs(&raw, &LinesConfig::default());
        assert_eq!(glyphs.len(), 1);
        assert_eq!(glyphs[0].color, [0; 3]);
        assert_eq!(glyphs[0].ox, 50.0);
        let mut copies = glyphs[0].extra.clone();
        copies.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(copies, vec![(49.5, 100.0), (50.5, 100.0)]);
    }

    /// A line glyph of 10 pt type with its advance from `x0` to `x1` and its
    /// ink from `ink_x0` to `ink_x1`.
    fn line_glyph_at(text: &str, (x0, x1): (f32, f32), (ink_x0, ink_x1): (f32, f32)) -> LineGlyph {
        LineGlyph {
            text: text.to_string(),
            x0,
            x1,
            y: 100.0,
            size: 10.0,
            font: 0,
            script: 0,
            space_before: false,
            ink: Rect::new(ink_x0, 93.0, ink_x1, 101.0),
            ox: x0,
            oy: 100.0,
            extra: Vec::new(),
            color: [0; 3],
        }
    }

    #[test]
    fn negation_slash_over_equals_is_not_equal() {
        // TeX's \neq: a slash of no advance whose ink crosses the "=" after it.
        let mut glyphs = vec![
            line_glyph_at("\u{0338}", (50.0, 50.0), (51.0, 56.0)),
            line_glyph_at("=", (50.0, 57.8), (50.6, 57.2)),
        ];
        compose_accents(&mut glyphs, &LinesConfig::default());
        assert_eq!(glyphs.len(), 1);
        assert_eq!(glyphs[0].text, "\u{2260}");
    }

    #[test]
    fn neighbouring_equal_letters_are_not_copies() {
        let raw = page(vec![glyph("l", 50.0, [0; 3]), glyph("l", 52.8, [0; 3])]);
        let (glyphs, _) = collect_glyphs(&raw, &LinesConfig::default());
        assert_eq!(glyphs.len(), 2);
    }

    #[test]
    fn close_glyphs_without_unicode_are_kept() {
        let raw = page(vec![glyph("\u{FFFD}", 50.0, [0; 3]), glyph("\u{FFFD}", 50.6, [0; 3])]);
        let (glyphs, _) = collect_glyphs(&raw, &LinesConfig::default());
        assert_eq!(glyphs.len(), 2);
    }
}
