//! Glyphs -> text lines.
//!
//! Glyphs are clustered by baseline, split at large horizontal gaps, and
//! sub/superscripts are attached to the line they belong to.

use crate::backend::{Dir, FontInfo, RawPage};
use crate::common::geom::Rect;
use crate::model::style::*;
use crate::model::{Line, LineGlyph, RotatedText};
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

impl Line {
    pub fn text(&self) -> String {
        let mut s = String::new();
        for g in &self.glyphs {
            if g.space_before && !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&g.text);
        }
        s
    }

    pub fn char_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Width of the first word (up to the first inter-word gap).
    pub fn first_word_width(&self) -> f32 {
        let mut end = self.bbox.x0;
        for (i, g) in self.glyphs.iter().enumerate() {
            if i > 0 && g.space_before {
                break;
            }
            end = end.max(g.x1);
        }
        end - self.bbox.x0
    }

    pub fn style_of(&self, g: &LineGlyph, fonts: &[FontInfo]) -> u8 {
        let f = &fonts[g.font as usize];
        let mut s = 0;
        if f.bold {
            s |= BOLD;
        }
        if f.italic {
            s |= ITALIC;
        }
        if f.mono {
            s |= MONO;
        }
        if f.math {
            s |= MATH;
        }
        if f.sans {
            s |= SANS;
        }
        match g.script {
            1 => s |= SUP,
            -1 => s |= SUB,
            _ => {}
        }
        s
    }

    /// Fraction of glyphs for which `pred` holds on the glyph's font.
    pub fn font_frac(&self, fonts: &[FontInfo], pred: impl Fn(&FontInfo) -> bool) -> f32 {
        if self.glyphs.is_empty() {
            return 0.0;
        }
        let n = self.glyphs.iter().filter(|g| pred(&fonts[g.font as usize])).count();
        n as f32 / self.glyphs.len() as f32
    }

    pub fn starts_with_superscript(&self) -> bool {
        self.glyphs.first().is_some_and(|g| g.script == 1)
    }
}

struct Seg {
    glyphs: Vec<LineGlyph>,
    y: f32,
    size: f32,
    x0: f32,
    x1: f32,
}

impl Seg {
    fn from_glyphs(glyphs: Vec<LineGlyph>) -> Self {
        let mut s = Seg {
            glyphs,
            y: 0.0,
            size: 0.0,
            x0: 0.0,
            x1: 0.0,
        };
        s.refresh();
        s
    }

    fn refresh(&mut self) {
        self.x0 = self.glyphs.iter().map(|g| g.x0).fold(f32::INFINITY, f32::min);
        self.x1 = self.glyphs.iter().map(|g| g.x1).fold(f32::NEG_INFINITY, f32::max);
        // Dominant size and baseline among the glyphs that sit on the baseline.
        let base: Vec<&LineGlyph> = self.glyphs.iter().filter(|g| g.script == 0).collect();
        let pool: Vec<&LineGlyph> = if base.is_empty() {
            self.glyphs.iter().collect()
        } else {
            base
        };
        self.size = crate::common::geom::weighted_mode(pool.iter().map(|g| (g.size, 1.0)), 0.25, 10.0);
        let size = self.size;
        let ys: Vec<f32> = pool
            .iter()
            .filter(|g| (g.size - size).abs() < 0.3)
            .map(|g| g.y)
            .collect();
        self.y = if ys.is_empty() {
            pool[0].y
        } else {
            crate::common::geom::median(&ys, pool[0].y)
        };
    }
}

fn combining_for(c: char) -> Option<char> {
    Some(match c {
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
        '\u{0300}'..='\u{036F}' => c,
        _ => return None,
    })
}

/// TeX's OT1 encoding draws accents as separate glyphs overlapping the base
/// letter; fold them back into precomposed characters.
fn compose_accents(glyphs: &mut Vec<LineGlyph>) {
    let mut i = 0;
    while i < glyphs.len() {
        let mut chars = glyphs[i].text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            i += 1;
            continue;
        };
        let Some(mark) = combining_for(c) else {
            i += 1;
            continue;
        };
        let a = &glyphs[i];
        let aw = (a.x1 - a.x0).max(0.5);
        let overlap = |b: &LineGlyph| -> f32 {
            let single_letter = b.text.chars().count() == 1 && b.text.chars().next().is_some_and(|c| c.is_alphabetic());
            if !single_letter || b.script != a.script {
                return 0.0;
            }
            (a.x1.min(b.x1) - a.x0.max(b.x0)) / aw.min((b.x1 - b.x0).max(0.5))
        };
        let next = if i + 1 < glyphs.len() {
            overlap(&glyphs[i + 1])
        } else {
            0.0
        };
        let prev = if i > 0 { overlap(&glyphs[i - 1]) } else { 0.0 };
        let target = if next >= 0.4 && next >= prev {
            Some(i + 1)
        } else if prev >= 0.4 {
            Some(i - 1)
        } else {
            None
        };
        let Some(t) = target else {
            i += 1;
            continue;
        };
        let base = match glyphs[t].text.as_str() {
            "\u{0131}" => "i".to_string(),
            "\u{0237}" => "j".to_string(),
            other => other.to_string(),
        };
        let composed: String = format!("{base}{mark}").nfc().collect();
        let (ax0, ax1) = (glyphs[i].x0, glyphs[i].x1);
        let ink = glyphs[i].ink;
        let accent_origin = (glyphs[i].ox, glyphs[i].oy);
        let g = &mut glyphs[t];
        g.extra = Some(accent_origin);
        g.text = composed;
        g.x0 = g.x0.min(ax0);
        g.x1 = g.x1.max(ax1);
        g.ink = g.ink.union(&ink);
        glyphs.remove(i);
        if t < i {
            // Removed an element after the target; stay at the same index.
            continue;
        }
    }
}

fn finalize(page: usize, mut seg: Seg) -> Line {
    seg.glyphs.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    compose_accents(&mut seg.glyphs);
    seg.refresh();
    let size = seg.size;
    let n = seg.glyphs.len();
    for i in 1..n {
        let (prev, cur) = (&seg.glyphs[i - 1], &seg.glyphs[i]);
        let reference = if prev.script != 0 && cur.script != 0 {
            prev.size.max(cur.size)
        } else {
            size
        };
        // Use the furthest right edge seen so far so that overlapping glyphs
        // (accents, stacked scripts) do not create spurious gaps.
        let right = seg.glyphs[..i]
            .iter()
            .rev()
            .take(3)
            .map(|g| g.x1)
            .fold(f32::NEG_INFINITY, f32::max);
        let gap = cur.x0 - right.max(prev.x1);
        let space = gap > 0.14 * reference;
        seg.glyphs[i].space_before = space;
    }
    if let Some(g) = seg.glyphs.first_mut() {
        g.space_before = false;
    }
    let ink = seg.glyphs.iter().fold(Rect::EMPTY, |acc, g| acc.union(&g.ink));
    let font = {
        let mut counts: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
        for g in seg.glyphs.iter().filter(|g| g.script == 0) {
            *counts.entry(g.font).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by_key(|&(f, c)| (c, std::cmp::Reverse(f)))
            .map(|(f, _)| f)
            .unwrap_or(seg.glyphs[0].font)
    };
    Line {
        page,
        bbox: Rect::new(seg.x0, seg.y - 0.75 * size, seg.x1, seg.y + 0.25 * size),
        ink,
        baseline: seg.y,
        size,
        font,
        glyphs: seg.glyphs,
    }
}

/// Merges segments that share a baseline and are separated by less than
/// `max_gap_em` (in units of the larger font size).
fn merge_same_baseline(mut segs: Vec<Seg>, max_gap_em: f32) -> Vec<Seg> {
    segs.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    let mut out: Vec<Seg> = Vec::with_capacity(segs.len());
    let i = 0;
    while i < segs.len() {
        // Collect the group of segments with (nearly) the same baseline.
        let mut j = i + 1;
        while j < segs.len() && segs[j].y - segs[i].y <= 0.25 + 0.03 * segs[i].size {
            j += 1;
        }
        let mut group: Vec<Seg> = segs.drain(i..j).collect();
        group.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let mut cur: Option<Seg> = None;
        for s in group {
            match cur.as_mut() {
                Some(c)
                    if s.x0 - c.x1 <= max_gap_em * c.size.max(s.size)
                        && (c.size / s.size).max(s.size / c.size) < 2.2 =>
                {
                    c.glyphs.extend(s.glyphs);
                    c.refresh();
                }
                _ => {
                    if let Some(c) = cur.take() {
                        out.push(c);
                    }
                    cur = Some(s);
                }
            }
        }
        if let Some(c) = cur {
            out.push(c);
        }
        // `drain` shifted the tail down, so `i` already points at the next group.
    }
    out
}

/// Merges fragments that belong to one visual row (e.g. a line interrupted by
/// an inline root or fraction) into a single line.
pub fn merge_row(page: usize, lines: Vec<Line>) -> Line {
    if lines.len() == 1 {
        return lines.into_iter().next().unwrap();
    }
    let main = lines
        .iter()
        .max_by_key(|l| l.glyphs.len())
        .map(|l| (l.baseline, l.size))
        .unwrap();
    let mut glyphs = Vec::new();
    for l in lines {
        let dy = l.baseline - main.0;
        let small = l.size < 0.9 * main.1;
        let flag = if !small {
            0
        } else if dy < -0.12 * main.1 {
            1
        } else if dy > 0.07 * main.1 {
            -1
        } else {
            0
        };
        for mut g in l.glyphs {
            if g.script == 0 {
                g.script = flag;
            }
            glyphs.push(g);
        }
    }
    finalize(page, Seg::from_glyphs(glyphs))
}

/// Splits a line at horizontal gaps of at least `min_gap` points. Used
/// inside tables, where neighbouring cells can be closer together than the
/// distance at which lines are normally broken.
pub fn split_at_gaps(line: &Line, min_gap: f32) -> Vec<Line> {
    let mut groups: Vec<Vec<LineGlyph>> = Vec::new();
    let mut right = f32::NEG_INFINITY;
    for g in &line.glyphs {
        if groups.is_empty() || g.x0 - right >= min_gap {
            groups.push(Vec::new());
        }
        groups.last_mut().unwrap().push(g.clone());
        right = right.max(g.x1);
    }
    if groups.len() <= 1 {
        return vec![line.clone()];
    }
    groups
        .into_iter()
        .map(|g| finalize(line.page, Seg::from_glyphs(g)))
        .collect()
}

pub fn build_lines(raw: &RawPage) -> (Vec<Line>, Vec<RotatedText>) {
    let has_visible = raw.glyphs.iter().any(|g| !g.invisible);
    let mut seen: HashSet<(i32, i32, u64)> = HashSet::new();
    let mut glyphs: Vec<LineGlyph> = Vec::new();
    let mut rotated: Vec<RotatedText> = Vec::new();
    let mut rot_last: Option<(f32, f32, f32, f32)> = None; // x, y, adv, angle

    for g in &raw.glyphs {
        if g.invisible && has_visible {
            continue;
        }
        if g.text.trim().is_empty() {
            continue;
        }
        if g.x < -5.0 || g.y < -5.0 || g.x > raw.width + 5.0 || g.y > raw.height + 5.0 {
            continue;
        }
        if g.dir == Dir::Rotated {
            // The glyph continues the current run if it lies on the same
            // baseline, right after the previous glyph.
            let (dir_x, dir_y) = (g.angle.to_radians().cos(), g.angle.to_radians().sin());
            let step = rot_last.map(|(x, y, adv, angle)| {
                let (dx, dy) = (g.x - x, g.y - y);
                let along = dx * dir_x + dy * dir_y;
                let across = (dx * dir_y - dy * dir_x).abs();
                (along, across, adv, angle)
            });
            let same = step.is_some_and(|(along, across, adv, angle)| {
                (angle - g.angle).abs() < 2.0
                    && across < 0.25 * g.size
                    && along > -0.3 * g.size
                    && along < adv + 1.2 * g.size
            });
            if same {
                let (along, _, adv, _) = step.unwrap();
                let r = rotated.last_mut().unwrap();
                if along > adv + 0.14 * g.size {
                    r.text.push(' ');
                }
                r.text.push_str(&g.text);
                r.bbox = r.bbox.union(&g.ink);
                r.length = ((g.x - r.x).powi(2) + (g.y - r.y).powi(2)).sqrt() + g.adv.max(0.0);
                r.origins.push((g.x, g.y));
            } else {
                rotated.push(RotatedText {
                    text: g.text.clone(),
                    bbox: g.ink,
                    x: g.x,
                    y: g.y,
                    angle: g.angle,
                    size: g.size,
                    length: g.adv.max(0.0),
                    font: g.font,
                    color: if g.color.iter().all(|&c| c < 48) {
                        [0; 3]
                    } else {
                        g.color
                    },
                    origins: vec![(g.x, g.y)],
                });
            }
            rot_last = Some((g.x, g.y, g.adv, g.angle));
            continue;
        }
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            g.text.hash(&mut h);
            ((g.x * 4.0).round() as i32, (g.y * 4.0).round() as i32, h.finish())
        };
        if !seen.insert(key) {
            continue;
        }
        glyphs.push(LineGlyph {
            text: g.text.clone(),
            x0: g.x,
            x1: g.x + g.adv.max(0.0),
            y: g.y,
            size: g.size,
            font: g.font,
            script: 0,
            space_before: false,
            ink: g.ink,
            ox: g.x,
            oy: g.y,
            extra: None,
            color: if g.color.iter().all(|&c| c < 48) {
                [0; 3]
            } else {
                g.color
            },
        });
    }

    // 1. Baseline groups, split at large gaps.
    let segs: Vec<Seg> = glyphs.into_iter().map(|g| Seg::from_glyphs(vec![g])).collect();
    let mut segs = merge_same_baseline(segs, 1.0);

    // Glyphs of math extension fonts (radicals, big operators, tall
    // delimiters) hang from their origin instead of sitting on the baseline.
    // Re-anchor such segments on the baseline implied by their ink, which is
    // centred on the math axis.
    for s in segs.iter_mut() {
        let ink = s.glyphs.iter().fold(Rect::EMPTY, |a, g| a.union(&g.ink));
        if ink.is_unset() {
            continue;
        }
        let axis = s.y - 0.25 * s.size;
        if (ink.cy() - axis).abs() > 0.5 * s.size {
            let y = ink.cy() + 0.25 * s.size;
            for g in s.glyphs.iter_mut() {
                g.y = y;
            }
            s.refresh();
        }
    }

    // 2. Attach sub/superscripts to their host line, smallest first.
    let mut order: Vec<usize> = (0..segs.len()).collect();
    order.sort_by(|&a, &b| segs[a].size.total_cmp(&segs[b].size));
    let mut slots: Vec<Option<Seg>> = segs.drain(..).map(Some).collect();
    for si in order {
        let Some(s) = slots[si].as_ref() else { continue };
        let (sy, ssize, sx0, sx1) = (s.y, s.size, s.x0, s.x1);
        let mut best: Option<(usize, f32)> = None;
        for (hi, h) in slots.iter().enumerate() {
            let Some(h) = h else { continue };
            if hi == si || h.size < 1.12 * ssize {
                continue;
            }
            let dy = sy - h.y;
            if dy < -0.62 * h.size || dy > 0.42 * h.size {
                continue;
            }
            let slack = 0.3 * h.size;
            if sx0 > h.x1 + slack || sx1 < h.x0 - slack {
                continue;
            }
            let score = dy.abs() / h.size;
            if best.is_none_or(|(_, b)| score < b) {
                best = Some((hi, score));
            }
        }
        if let Some((hi, _)) = best {
            let s = slots[si].take().unwrap();
            let h = slots[hi].as_mut().unwrap();
            let dy = s.y - h.y;
            let flag = if dy < -0.12 * h.size {
                1
            } else if dy > 0.07 * h.size {
                -1
            } else {
                0
            };
            for mut g in s.glyphs {
                // A script of a script stays inside its parent's level.
                if flag != 0 {
                    g.script = flag;
                }
                h.glyphs.push(g);
            }
            h.x0 = h.x0.min(s.x0);
            h.x1 = h.x1.max(s.x1);
        }
    }
    let segs: Vec<Seg> = slots.into_iter().flatten().collect();

    // 3. Hosts may have grown towards each other.
    let segs = merge_same_baseline(segs, 1.0);

    let mut lines: Vec<Line> = segs.into_iter().map(|s| finalize(raw.index, s)).collect();
    lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
    (lines, rotated)
}
