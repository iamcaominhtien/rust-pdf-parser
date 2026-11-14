//! Reading a picture of a formula: its pieces of ink, recognised as glyphs
//! of Latin Modern Math and placed as glyphs on a PDF page would be, for the
//! formula builder.

use super::shape::{Mask, Shape};
use super::templates::{Kind, TEMPLATES, Template, font};
use crate::common::geom::{Rect, median};
use crate::model::LineGlyph;

/// Thresholds of the reading of formula pictures.
#[derive(Debug, Clone)]
pub struct RasterConfig {
    /// Grey level (0 black, 255 white) below which a pixel is ink.
    pub ink_max_level: u8,
    /// Pieces of at most this many pixels are specks of the picture, not
    /// ink.
    pub speck_max_pixels: usize,
    /// Font size the main text of the formula is given, in points.
    pub main_size_pt: f32,
    /// How much a difference of proportions counts against a template,
    /// per unit of the logarithm of their ratio.
    pub aspect_weight_ratio: f32,
    /// How much a size unlike the text or script sizes counts against a
    /// letter or symbol, per unit of the logarithm of the ratio.
    pub size_weight_ratio: f32,
    /// A piece of ink taller than this (in sizes of the text) is a tall
    /// delimiter or a big operator: its proportions count less.
    pub tall_min_height_em: f32,
    /// A piece at most this thick, and `bar_min_aspect_ratio` times longer,
    /// is a bar: a minus sign, part of `=`, or a rule.
    pub bar_max_thickness_em: f32,
    /// See `bar_max_thickness_em`.
    pub bar_min_aspect_ratio: f32,
    /// A bar covers at least this fraction of its box with ink.
    pub bar_min_fill_ratio: f32,
    /// A bar with ink this close right above or below it is a fraction
    /// bar or an overline (a rule); otherwise it is a minus sign.
    pub bar_clear_em: f32,
    /// A piece at most this large either way is a dot.
    pub dot_max_em: f32,
    /// Pieces this close one above the other may be one glyph (`i`, `=`,
    /// `≤`)...
    pub merge_max_gap_em: f32,
    /// ... when they overlap horizontally by this fraction of the narrower.
    pub merge_min_overlap_ratio: f32,
    /// Least likeness of a template for a piece to be read as it; a piece
    /// below it makes the formula unreadable.
    pub min_score: f32,
    /// A piece at most this large either way is small punctuation (`,`,
    /// `′`), read among the small symbols only, whose shape at this size
    /// says little: ...
    pub small_max_em: f32,
    /// ... it needs only this likeness.
    pub small_min_score: f32,
    /// The bar of a radical sign takes at least this fraction of the width
    /// of the piece.
    pub radical_min_bar_ratio: f32,
    /// How much an amount of ink unlike the template's counts against it
    /// (per unit of the logarithm of the ratio): bold or not.
    pub weight_weight_ratio: f32,
    /// A letter read at the size of a script whose baseline is within this
    /// distance of a line of the formula is not a script...
    pub script_baseline_tolerance_em: f32,
    /// ... and is given this penalty.
    pub script_on_baseline_penalty: f32,
    /// Penalty of rare glyphs (bold and Fraktur letters, [`RARE`]) against
    /// common ones that look alike.
    pub rare_penalty: f32,
}

impl Default for RasterConfig {
    fn default() -> Self {
        Self {
            ink_max_level: 200,
            speck_max_pixels: 2,
            main_size_pt: 10.0,
            aspect_weight_ratio: 0.35,
            size_weight_ratio: 0.3,
            tall_min_height_em: 1.4,
            bar_max_thickness_em: 0.12,
            bar_min_aspect_ratio: 3.0,
            bar_min_fill_ratio: 0.7,
            bar_clear_em: 0.4,
            dot_max_em: 0.16,
            merge_max_gap_em: 0.35,
            merge_min_overlap_ratio: 0.5,
            min_score: 0.45,
            small_max_em: 0.4,
            small_min_score: 0.0,
            radical_min_bar_ratio: 0.3,
            weight_weight_ratio: 0.4,
            script_baseline_tolerance_em: 0.08,
            script_on_baseline_penalty: 0.3,
            rare_penalty: 0.05,
        }
    }
}

/// Symbols that look like more common ones (`υ` like `v`, `ο` like `o`,
/// `⌊` like `[`).
const RARE: &[&str] = &["υ", "ο", "ϖ", "ϱ", "ϑ", "⌊", "⌋", "⌈", "⌉", "⊙", "∘", "Ξ", "∣", "ℓ"];

/// A symbol this many times wider than high is flat: its size is read
/// from its width.
const FLAT_MIN_ASPECT: f32 = 1.5;

/// Least coverage of the pixels that make the box of a piece.
const CORE_MIN_COVER: f32 = 0.4;

/// The small symbols a small piece is read among.
const SMALL: &[&str] = &[
    ".", ",", "′", "'", "˙", "`", "ˆ", "˜", "¯", "¨", "ˇ", "˘", "−", "∘", "·",
];

/// Sizes of letters relative to the text: text, script, script of script.
const LEVELS: [f32; 3] = [1.0, 0.7, 0.5];

/// How much heavier the strokes of letters at each of the [`LEVELS`] are
/// than those of the text font scaled to that size.
const SCRIPT_WEIGHT_GAIN: [f32; 3] = [1.0, 1.2, 1.35];

/// The glyphs and rules read from a picture, in points.
#[derive(Debug, Clone, Default)]
pub struct Reading {
    /// The glyphs, as a PDF would give them.
    pub glyphs: Vec<LineGlyph>,
    /// Horizontal rules (fraction bars, overlines, radical bars).
    pub rules: Vec<Rect>,
}

/// A piece of ink of the picture (one or more connected pieces read as one
/// glyph), in pixels.
#[derive(Debug, Clone)]
struct Piece {
    /// Its pixels (column, row, how much of it is ink).
    pixels: Vec<(usize, usize, f32)>,
    /// Its box (right and bottom edges excluded).
    rect: Rect,
    /// The template decided before matching (a radical sign, `=`, the
    /// pieces of `i` or `≤`), as an index into [`TEMPLATES`].
    forced: Option<usize>,
}

impl Piece {
    /// A piece of the pixels `pixels`.
    fn new(pixels: Vec<(usize, usize, f32)>) -> Self {
        // The box of the core of the ink: the faint edge of anti-aliasing
        // would shift a thin glyph against its template.
        let core = |min: f32| {
            pixels
                .iter()
                .filter(|&&(.., cover)| cover >= min)
                .fold(Rect::EMPTY, |rect, &(x, y, _)| {
                    rect.union(&Rect::new(x as f32, y as f32, x as f32 + 1.0, y as f32 + 1.0))
                })
        };
        let mut rect = core(CORE_MIN_COVER);
        if rect.is_unset() {
            rect = core(0.0);
        }
        Self {
            pixels,
            rect,
            forced: None,
        }
    }

    /// The two pieces as one.
    fn joined(&self, other: &Piece) -> Self {
        Self::new(self.pixels.iter().chain(&other.pixels).copied().collect())
    }

    /// Its ink as a mask.
    fn mask(&self) -> Mask {
        let (x0, y0) = (self.rect.x0 as usize, self.rect.y0 as usize);
        let (width, height) = (self.rect.width() as usize, self.rect.height() as usize);
        let mut ink = vec![0.0; width * height];
        for &(x, y, cover) in &self.pixels {
            if x >= x0 && y >= y0 && x - x0 < width && y - y0 < height {
                ink[(y - y0) * width + (x - x0)] = cover;
            }
        }
        Mask::new(ink, width, height, self.rect.x0, self.rect.y0)
    }
}

/// The template a piece is read as.
#[derive(Debug, Clone, Copy)]
struct Match {
    /// Index into [`TEMPLATES`].
    template: usize,
    /// How well it matches.
    score: f32,
    /// Size of the em the piece implies, in pixels.
    em_px: f32,
    /// The size level it is set at (see [`LEVELS`]).
    level: f32,
}

/// Reads the glyphs of the formula pictured in `gray` (grey levels, row by
/// row, `width` wide); `None` when some piece cannot be read.
pub fn read(gray: &[u8], width: usize, cfg: &RasterConfig) -> Option<Reading> {
    let height = gray.len() / width.max(1);
    let cover = |level: u8| f32::from(255 - level) / 255.0;
    let ink: Vec<f32> = gray
        .iter()
        .map(|&level| if level < cfg.ink_max_level { cover(level) } else { 0.0 })
        .collect();
    let mut pieces: Vec<Piece> = Mask::new(ink, width, height, 0.0, 0.0)
        .pieces()
        .into_iter()
        .filter(|pixels| pixels.len() > cfg.speck_max_pixels)
        .map(|pixels| {
            Piece::new(
                pixels
                    .into_iter()
                    .map(|(x, y)| (x, y, cover(gray[y * width + x])))
                    .collect(),
            )
        })
        .collect();
    if pieces.is_empty() || TEMPLATES.is_empty() {
        return None;
    }
    let em = text_em(&pieces, cfg);
    let radical_bars = split_radicals(&mut pieces, em, cfg);
    join_bars(&mut pieces, em, cfg);
    join_pieces(&mut pieces, em, cfg);
    let rows = text_rows(&pieces, em, cfg);
    let mut placed: Vec<(Piece, Option<Match>)> = Vec::new();
    for piece in pieces {
        if is_bar(&piece, em, cfg) && piece.forced.is_none() {
            placed.push((piece, None));
            continue;
        }
        let mut found = classify(&piece, Some(em), &rows, cfg, |_| true)?;
        // Small punctuation has little shape at this size: read among the
        // small symbols, it needs less likeness.
        let small = found.score < cfg.min_score && piece.rect.width().max(piece.rect.height()) <= cfg.small_max_em * em;
        if small {
            found = classify(&piece, Some(em), &rows, cfg, |template| {
                SMALL.contains(&template.text.as_str())
            })?;
        }
        if found.score < if small { cfg.small_min_score } else { cfg.min_score } {
            return None;
        }
        placed.push((piece, Some(found)));
    }
    let scale = cfg.main_size_pt / em;
    let mut rules: Vec<Rect> = radical_bars.iter().map(|rule| scaled(rule, scale)).collect();
    let others: Vec<Rect> = placed.iter().map(|(piece, _)| piece.rect).collect();
    let mut glyphs = Vec::new();
    for (idx, (piece, found)) in placed.iter().enumerate() {
        match found {
            Some(found) => glyphs.push(glyph(piece, found, em, scale)),
            None if clear_bar(&piece.rect, idx, &others, em, cfg) => glyphs.push(minus(piece, em, scale)),
            None => rules.push(scaled(&piece.rect, scale)),
        }
    }
    fix_positions(&mut glyphs, cfg.main_size_pt);
    Some(Reading { glyphs, rules })
}

/// The size of the em of the text of the formula, in pixels: the median of
/// what the letters read without regard to size imply.
fn text_em(pieces: &[Piece], cfg: &RasterConfig) -> f32 {
    let implied: Vec<f32> = pieces
        .iter()
        .filter_map(|piece| classify(piece, None, &[], cfg, |template| template.kind == Kind::Letter))
        .filter(|found| found.score >= cfg.min_score)
        .map(|found| found.em_px)
        .collect();
    let heights: Vec<f32> = pieces.iter().map(|piece| piece.rect.height() / 0.7).collect();
    median(&implied, median(&heights, 10.0))
}

/// The index of the template of `text`.
fn template_of(text: &str) -> Option<usize> {
    TEMPLATES.iter().position(|template| template.text == text)
}

/// The best template for `piece` among those `allowed`, judged by shape
/// and proportions and, when the size of the text `em` (pixels) is known,
/// by size.
fn classify(
    piece: &Piece,
    em: Option<f32>,
    rows: &[f32],
    cfg: &RasterConfig,
    allowed: impl Fn(&Template) -> bool,
) -> Option<Match> {
    let height = piece.rect.height();
    if let Some(template) = piece.forced {
        let em_px = implied_em(piece, &TEMPLATES[template]);
        let (_, level) = size_penalty(&TEMPLATES[template], em_px, em);
        return Some(Match {
            template,
            score: 1.0,
            em_px,
            level,
        });
    }
    if let Some(em) = em
        && piece.rect.width().max(height) <= cfg.dot_max_em * em
    {
        let template = template_of(".")?;
        return Some(Match {
            template,
            score: 1.0,
            em_px: em,
            level: 1.0,
        });
    }
    let shape = Shape::of(&piece.mask());
    let ink: f32 = piece.pixels.iter().map(|&(.., cover)| cover).sum();
    let tall = em.is_some_and(|em| height > cfg.tall_min_height_em * em);
    TEMPLATES
        .iter()
        .enumerate()
        .filter(|(_, template)| allowed(template))
        .filter(|(_, template)| !tall || matches!(template.kind, Kind::Delimiter | Kind::BigOp))
        .map(|(idx, template)| {
            let aspect = if tall { 0.3 } else { 1.0 } * cfg.aspect_weight_ratio;
            let em_px = implied_em(piece, template);
            let (penalty, level) = size_penalty(template, em_px, em);
            let baseline = piece.rect.y1 - template.bottom_em * em_px;
            let on_line = em.is_some_and(|em| {
                rows.iter()
                    .any(|row| (row - baseline).abs() <= cfg.script_baseline_tolerance_em * em)
            });
            // Scripts are set in fonts of their own size (cmmi7, cmmi5),
            // with heavier strokes than the text font scaled down.
            let gain = LEVELS
                .iter()
                .zip(SCRIPT_WEIGHT_GAIN)
                .find(|(size, _)| **size == level)
                .map_or(1.0, |(_, gain)| gain);
            let weight = (ink / (em_px * em_px) / (gain * template.ink_em2).max(1e-4)).ln().abs();
            let rare = if matches!(template.font, font::BOLD | font::BOLD_ITALIC | font::FRAKTUR)
                || RARE.contains(&template.text.as_str())
            {
                cfg.rare_penalty
            } else {
                0.0
            };
            let score = shape.likeness(&template.shape)
                - aspect * (shape.aspect / template.shape.aspect).ln().abs()
                - cfg.size_weight_ratio * penalty
                - cfg.weight_weight_ratio * weight
                - if level < 1.0 && on_line {
                    cfg.script_on_baseline_penalty
                } else {
                    0.0
                }
                - rare;
            Match {
                template: idx,
                score,
                em_px,
                level,
            }
        })
        .max_by(|a, b| a.score.total_cmp(&b.score))
}

/// The size of the em, in pixels, that reading `piece` as `template`
/// implies: from the heights, or for flat symbols (`=`, `→`) whose height
/// varies little with the size, from the widths.
fn implied_em(piece: &Piece, template: &Template) -> f32 {
    if template.kind == Kind::Symbol && template.shape.aspect > FLAT_MIN_ASPECT {
        let width_em = template.shape.aspect * template.height_em();
        piece.rect.width() / width_em.max(0.01)
    } else {
        piece.rect.height() / template.height_em().max(0.01)
    }
}

/// How far the em `em_px` a template implies is from the sizes it may be
/// set at (the logarithm of the ratio), and the nearest of them.
fn size_penalty(template: &Template, em_px: f32, em: Option<f32>) -> (f32, f32) {
    let Some(em) = em else { return (0.0, 1.0) };
    match template.kind {
        Kind::Letter | Kind::Symbol | Kind::Accent => LEVELS
            .iter()
            .map(|&level| ((em_px / (em * level)).ln().abs(), level))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap_or((0.0, 1.0)),
        // Big operators and delimiters may be larger than the text, not
        // smaller than a script.
        Kind::BigOp | Kind::Delimiter => ((em_px / (em * LEVELS[1])).ln().min(0.0).abs(), 1.0),
    }
}

/// Whether `piece` is a bar: thin, long and straight (it fills its box,
/// unlike a tilde).
fn is_bar(piece: &Piece, em: f32, cfg: &RasterConfig) -> bool {
    let (width, height) = (piece.rect.width(), piece.rect.height());
    let filled = piece.pixels.len() as f32 >= cfg.bar_min_fill_ratio * width * height;
    height <= (cfg.bar_max_thickness_em * em).max(1.0) && width >= cfg.bar_min_aspect_ratio * height && filled
}

/// Splits the radical signs drawn in one piece with their bar into the sign
/// and the bar (a rule, returned, in pixels): a long thin top-right part
/// with no ink under it in the piece.
fn split_radicals(pieces: &mut [Piece], em: f32, cfg: &RasterConfig) -> Vec<Rect> {
    let mut bars = Vec::new();
    let Some(sign) = template_of("√") else { return bars };
    let band = (cfg.bar_max_thickness_em * em).ceil().max(1.0) as usize + 1;
    for piece in pieces.iter_mut() {
        let (width, height) = (piece.rect.width() as usize, piece.rect.height() as usize);
        if (height as f32) < 0.7 * em || width < height {
            continue;
        }
        let mask = piece.mask();
        let in_band = |x: usize| (0..height).all(|y| !mask.at(x, y) || y < band) && (0..band).any(|y| mask.at(x, y));
        let run = (0..width).rev().take_while(|&x| in_band(x)).count();
        let start = width - run;
        if (run as f32) < cfg.radical_min_bar_ratio * width as f32 || (start as f32) < 0.3 * em {
            continue;
        }
        let x0 = piece.rect.x0;
        let thickness = (start..width)
            .filter_map(|x| (0..band).rposition(|y| mask.at(x, y)))
            .max()
            .unwrap_or(0)
            + 1;
        bars.push(Rect::new(
            x0 + start as f32,
            piece.rect.y0,
            piece.rect.x1,
            piece.rect.y0 + thickness as f32,
        ));
        let pixels: Vec<(usize, usize, f32)> = piece
            .pixels
            .iter()
            .copied()
            .filter(|&(x, ..)| (x as f32) < x0 + start as f32)
            .collect();
        *piece = Piece::new(pixels);
        piece.forced = Some(sign);
    }
    bars
}

/// Joins bars of the same length right above one another into `=` (two)
/// and `≡` (three).
fn join_bars(pieces: &mut Vec<Piece>, em: f32, cfg: &RasterConfig) {
    let (Some(equals), Some(identical)) = (template_of("="), template_of("≡")) else {
        return;
    };
    let tol = 0.15 * em;
    loop {
        let bars: Vec<usize> = (0..pieces.len())
            .filter(|&idx| is_bar(&pieces[idx], em, cfg) || pieces[idx].forced == Some(equals))
            .collect();
        let pair = bars
            .iter()
            .flat_map(|&a| bars.iter().map(move |&b| (a, b)))
            .find(|&(a, b)| {
                let (upper, lower) = (&pieces[a].rect, &pieces[b].rect);
                let gap = lower.y0 - upper.y1;
                let between = Rect::new(upper.x0, upper.y1, upper.x1, lower.y0);
                a != b
                    && (upper.x0 - lower.x0).abs() <= tol
                    && (upper.x1 - lower.x1).abs() <= tol
                    && gap > 0.0
                    && gap <= 0.4 * em
                    && upper.width() <= 1.3 * em
                    && !pieces.iter().any(|piece| piece.rect.intersects(&between))
            });
        let Some((a, b)) = pair else { return };
        let three = pieces[a].forced == Some(equals) || pieces[b].forced == Some(equals);
        let mut joined = pieces[a].joined(&pieces[b]);
        joined.forced = Some(if three { identical } else { equals });
        let (first, second) = (a.max(b), a.min(b));
        pieces.remove(first);
        pieces[second] = joined;
    }
}

/// Joins pieces right above one another that read better as one glyph of
/// several pieces (`i`, `j`, `≤`, `÷`, `:`) than apart.
fn join_pieces(pieces: &mut Vec<Piece>, em: f32, cfg: &RasterConfig) {
    let several = |template: &Template| template.parts >= 2;
    loop {
        let mut best: Option<(f32, usize, usize, Piece)> = None;
        for a in 0..pieces.len() {
            for b in a + 1..pieces.len() {
                let (one, two) = (&pieces[a], &pieces[b]);
                if one.forced.is_some() || two.forced.is_some() {
                    continue;
                }
                let narrower = one.rect.width().min(two.rect.width());
                let gap = (two.rect.y0 - one.rect.y1).max(one.rect.y0 - two.rect.y1);
                if one.rect.x_overlap(&two.rect) < cfg.merge_min_overlap_ratio * narrower
                    || gap > cfg.merge_max_gap_em * em
                {
                    continue;
                }
                let joined = one.joined(two);
                let Some(found) = classify(&joined, Some(em), &[], cfg, several) else {
                    continue;
                };
                // A dot alone (of `i`, `:`) is no evidence against joining.
                let alone = |piece: &Piece| {
                    if piece.rect.width().max(piece.rect.height()) <= cfg.small_max_em * em {
                        return cfg.min_score;
                    }
                    classify(piece, Some(em), &[], cfg, |_| true).map_or(0.0, |found| found.score)
                };
                let apart = (alone(one) + alone(two)) / 2.0;
                if found.score >= cfg.min_score + 0.15
                    && found.score >= apart - 0.05
                    && best.as_ref().is_none_or(|(score, ..)| found.score > *score)
                {
                    let mut joined = joined;
                    joined.forced = Some(found.template);
                    best = Some((found.score, a, b, joined));
                }
            }
        }
        let Some((_, a, b, joined)) = best else { return };
        pieces.remove(b);
        pieces[a] = joined;
    }
}

/// Whether the bar `rect` (the piece `idx` of `others`) has no ink right
/// above or below it: a minus sign rather than a fraction bar.
fn clear_bar(rect: &Rect, idx: usize, others: &[Rect], em: f32, cfg: &RasterConfig) -> bool {
    let reach = cfg.bar_clear_em * em;
    !others.iter().enumerate().any(|(other, ink)| {
        other != idx
            && ink.x_overlap(rect) > 0.0
            && ((rect.y0 - ink.y1 >= 0.0 && rect.y0 - ink.y1 <= reach)
                || (ink.y0 - rect.y1 >= 0.0 && ink.y0 - rect.y1 <= reach))
    })
}

/// `rect` (pixels) in points, at `scale` points per pixel.
fn scaled(rect: &Rect, scale: f32) -> Rect {
    Rect::new(rect.x0 * scale, rect.y0 * scale, rect.x1 * scale, rect.y1 * scale)
}

/// The glyph `piece` is read as.
fn glyph(piece: &Piece, found: &Match, em: f32, scale: f32) -> LineGlyph {
    let template = &TEMPLATES[found.template];
    let size = match template.kind {
        Kind::BigOp | Kind::Delimiter => em,
        _ => em * found.level,
    };
    let baseline = piece.rect.y1 - template.bottom_em * found.em_px;
    let mut glyph = placed_glyph(&template.text, template.font, &piece.rect, baseline, size, scale);
    // The advance box, as a PDF gives it: the ink with the side bearings.
    glyph.x0 -= template.before_em.max(0.0) * found.em_px * scale;
    glyph.x1 += template.after_em.max(0.0) * found.em_px * scale;
    glyph
}

/// The minus sign drawn as the bar `piece`, on the math axis.
fn minus(piece: &Piece, em: f32, scale: f32) -> LineGlyph {
    let bottom = TEMPLATES
        .iter()
        .find(|template| template.text == "−")
        .map_or(-0.22, |template| template.bottom_em);
    placed_glyph("−", font::SYMBOL, &piece.rect, piece.rect.y1 - bottom * em, em, scale)
}

/// A glyph of `text` in `font` with the ink `rect`, on `baseline`, of size
/// `size` (pixels), turned into points by `scale`.
fn placed_glyph(text: &str, font: u16, rect: &Rect, baseline: f32, size: f32, scale: f32) -> LineGlyph {
    let ink = scaled(rect, scale);
    LineGlyph {
        text: text.to_string(),
        x0: ink.x0,
        x1: ink.x1,
        y: baseline * scale,
        size: size * scale,
        font,
        script: 0,
        space_before: false,
        ink,
        ox: ink.x0,
        oy: baseline * scale,
        extra: None,
        color: [0; 3],
    }
}

/// Tells dots on the baseline (`.`) from dots on the math axis (`·`), and
/// commas from primes, by where they are against the letters next to them.
fn fix_positions(glyphs: &mut [LineGlyph], main: f32) {
    let letters: Vec<(Rect, f32, f32)> = glyphs
        .iter()
        .filter(|glyph| glyph.size >= 0.85 * main && glyph.text.chars().all(char::is_alphanumeric))
        .map(|glyph| (glyph.ink, glyph.y, glyph.size))
        .collect();
    for glyph in glyphs.iter_mut() {
        if !matches!(glyph.text.as_str(), "." | "·" | "," | "′" | "'") {
            continue;
        }
        let ink = glyph.ink;
        let Some(&(_, baseline, size)) = letters
            .iter()
            .filter(|(_, baseline, _)| (baseline - ink.cy()).abs() <= 0.8 * main)
            .min_by(|a, b| (a.0.cx() - ink.cx()).abs().total_cmp(&(b.0.cx() - ink.cx()).abs()))
        else {
            continue;
        };
        let (text, font) = match glyph.text.as_str() {
            "." | "·" if ink.cy() < baseline - 0.15 * size => ("·", font::SYMBOL),
            "." | "·" => (".", font::ROMAN),
            _ if ink.y1 > baseline - 0.1 * size => (",", font::ROMAN),
            _ => ("′", font::SYMBOL),
        };
        glyph.text = text.to_string();
        glyph.font = font;
        glyph.y = baseline;
    }
}

/// The baselines of the lines of text of the formula (rows, numerators,
/// denominators): those of the letters read at the size of the text with
/// confidence.
fn text_rows(pieces: &[Piece], em: f32, cfg: &RasterConfig) -> Vec<f32> {
    let mut rows: Vec<f32> = Vec::new();
    for piece in pieces
        .iter()
        .filter(|piece| piece.forced.is_none() && !is_bar(piece, em, cfg))
    {
        let Some(found) = classify(piece, Some(em), &[], cfg, |template| template.kind == Kind::Letter) else {
            continue;
        };
        let template = &TEMPLATES[found.template];
        if found.level < 1.0 || found.score < 0.8 {
            continue;
        }
        let baseline = piece.rect.y1 - template.bottom_em * found.em_px;
        if !rows
            .iter()
            .any(|row| (row - baseline).abs() <= cfg.script_baseline_tolerance_em * em)
        {
            rows.push(baseline);
        }
    }
    rows
}
