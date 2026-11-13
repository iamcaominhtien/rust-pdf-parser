//! Glyphs -> text lines.
//!
//! Glyphs are clustered by baseline, split at large horizontal gaps, and
//! sub/superscripts are attached to the line they belong to.

use crate::backend::{Dir, FontInfo, Glyph, RawPage};
use crate::common::geom::{Rect, median, weighted_mode};
use crate::model::style::{BOLD, ITALIC, MATH, MONO, SANS, SUB, SUP};
use crate::model::{Line, LineGlyph, RotatedText};
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

/// The default thresholds, as a constant so that [`merge_row`] and
/// [`split_at_gaps`] (called from later stages that do not hold a
/// [`LinesConfig`]) build their lines with the same values as
/// [`build_lines`] does by default.
const DEFAULT_CONFIG: LinesConfig = LinesConfig {
    page_margin_pt: 5.0,
    near_black_max_value: 48,
    duplicate_grid_per_pt: 4.0,
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
    /// Resolution (cells per point) of the grid on which glyph origins are
    /// compared to find duplicates: the same text drawn twice within a
    /// quarter point (fake bold, shadows) is kept once.
    pub duplicate_grid_per_pt: f32,
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
        if font.mono {
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
    finalize(page, Segment::from_glyphs(glyphs, cfg), cfg)
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

    let mut lines: Vec<Line> = segments
        .into_iter()
        .map(|segment| finalize(raw.index, segment, cfg))
        .collect();
    lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
    (lines, rotated)
}

/// Reads the glyphs of a page: drops hidden, blank, off-page and duplicate
/// glyphs, collects rotated glyphs into runs, and turns the remaining ones
/// into line glyphs (in drawing order).
fn collect_glyphs(raw: &RawPage, cfg: &LinesConfig) -> (Vec<LineGlyph>, Vec<RotatedText>) {
    let has_visible = raw.glyphs.iter().any(|glyph| !glyph.invisible);
    let mut seen: HashSet<(i32, i32, u64)> = HashSet::new();
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
        if !seen.insert(duplicate_key(glyph, cfg)) {
            continue;
        }
        glyphs.push(LineGlyph {
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
            extra: None,
            color: display_color(glyph.color, cfg),
        });
    }
    (glyphs, rotated.runs)
}

/// Whether the glyph's origin lies clearly outside the page.
fn is_off_page(glyph: &Glyph, raw: &RawPage, cfg: &LinesConfig) -> bool {
    glyph.x < -cfg.page_margin_pt
        || glyph.y < -cfg.page_margin_pt
        || glyph.x > raw.width + cfg.page_margin_pt
        || glyph.y > raw.height + cfg.page_margin_pt
}

/// Key identifying a glyph drawn twice at (nearly) the same place: its
/// origin on a fine grid and a hash of its text.
fn duplicate_key(glyph: &Glyph, cfg: &LinesConfig) -> (i32, i32, u64) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    glyph.text.hash(&mut hasher);
    (
        (glyph.x * cfg.duplicate_grid_per_pt).round() as i32,
        (glyph.y * cfg.duplicate_grid_per_pt).round() as i32,
        hasher.finish(),
    )
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
        let Some(target) = accent_target(glyphs, i, cfg) else {
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
        let letter = &mut glyphs[target];
        letter.extra = Some(accent_origin);
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
    mark_spaces(&mut segment.glyphs, size, cfg);
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
    }
}

/// Sets `space_before` on every glyph (sorted left to right) that follows a
/// gap wider than a word space. Between two scripts the gap is measured
/// against the larger script size, otherwise against the line size `size`.
fn mark_spaces(glyphs: &mut [LineGlyph], size: f32, cfg: &LinesConfig) {
    let n = glyphs.len();
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
        let gap = cur.x0 - right.max(prev.x1);
        let space = gap > cfg.space_gap_em * reference;
        glyphs[i].space_before = space;
    }
    if let Some(glyph) = glyphs.first_mut() {
        glyph.space_before = false;
    }
}

/// Merges segments that share a baseline and are separated by less than
/// [`LinesConfig::merge_max_gap_em`] (in units of the larger font size).
fn merge_same_baseline(mut segments: Vec<Segment>, cfg: &LinesConfig) -> Vec<Segment> {
    segments.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let mut out: Vec<Segment> = Vec::with_capacity(segments.len());
    let i = 0;
    while i < segments.len() {
        // Collect the group of segments with (nearly) the same baseline.
        let mut j = i + 1;
        while j < segments.len()
            && segments[j].y - segments[i].y <= cfg.same_baseline_pt + cfg.same_baseline_em * segments[i].size
        {
            j += 1;
        }
        let mut group: Vec<Segment> = segments.drain(i..j).collect();
        group.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let mut cur: Option<Segment> = None;
        for segment in group {
            match cur.as_mut() {
                Some(current)
                    if segment.x0 - current.x1 <= cfg.merge_max_gap_em * current.size.max(segment.size)
                        && (current.size / segment.size).max(segment.size / current.size)
                            < cfg.merge_max_size_factor =>
                {
                    current.glyphs.extend(segment.glyphs);
                    current.refresh(cfg);
                }
                _ => {
                    if let Some(current) = cur.take() {
                        out.push(current);
                    }
                    cur = Some(segment);
                }
            }
        }
        if let Some(current) = cur {
            out.push(current);
        }
        // `drain` shifted the tail down, so `i` already points at the next group.
    }
    out
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
