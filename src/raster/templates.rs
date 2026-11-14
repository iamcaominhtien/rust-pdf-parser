//! The glyphs a picture of a formula is compared with: the glyphs of the
//! TeX fonts formulas are set in, at their sizes, drawn once from their
//! outlines.

use super::shape::{Mask, Shape};
use super::tex::{self, TEX_FONTS};
use skrifa::outline::OutlinePen;
use skrifa::{DrawMemory, Font, ScalerFont};
use std::sync::LazyLock;

/// Indices into the font table the rebuilt glyphs refer to (see
/// [`super::fonts`]): the TeX fonts each symbol comes from in papers.
pub mod font {
    /// Math italic letters (`cmmi`).
    pub const ITALIC: u16 = 0;
    /// Upright letters, digits and some symbols (`cmr`).
    pub const ROMAN: u16 = 1;
    /// Symbols and calligraphic letters (`cmsy`).
    pub const SYMBOL: u16 = 2;
    /// Big operators (`cmex`).
    pub const EXTENSION: u16 = 3;
    /// Blackboard bold letters (`msbm`).
    pub const DOUBLE_STRUCK: u16 = 4;
    /// Bold letters and digits (`cmbx`).
    pub const BOLD: u16 = 5;
    /// Bold italic letters (`cmmib`).
    pub const BOLD_ITALIC: u16 = 6;
    /// Fraktur letters (`eufm`).
    pub const FRAKTUR: u16 = 7;
    /// Script capitals (`rsfs`).
    pub const SCRIPT: u16 = 8;
}

/// What a template is, for the way its size is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A letter or a digit, set at the size of the text or of a script.
    Letter,
    /// An operator, a relation or punctuation.
    Symbol,
    /// A big operator, larger in displays.
    BigOp,
    /// A delimiter or a radical sign, which grows with what it holds.
    Delimiter,
    /// An accent over a letter.
    Accent,
}

/// A symbol drawn from the font.
#[derive(Debug, Clone)]
pub struct Template {
    /// The text of the glyph, as in a PDF (the plain letter: the style is
    /// in the font).
    pub text: String,
    /// The font of the glyph (see [`font`]).
    pub font: u16,
    /// What it is.
    pub kind: Kind,
    /// Top of the ink, in ems below the baseline (negative: above it).
    pub top_em: f32,
    /// Bottom of the ink, in ems below the baseline.
    pub bottom_em: f32,
    /// Its normalised shape.
    pub shape: Shape,
    /// Its number of separate pieces of ink (2 for `i`, `=`, `≤`).
    pub parts: usize,
    /// The size of its font against the text (0.7 for the script fonts).
    pub level: f32,
    /// Its amount of ink, in square ems (bold glyphs have more).
    pub ink_em2: f32,
    /// Space before the ink, in ems (the left side bearing).
    pub before_em: f32,
    /// Space after the ink, in ems (the advance less the ink and the
    /// space before it).
    pub after_em: f32,
}

impl Template {
    /// Height of the ink in ems.
    pub fn height_em(&self) -> f32 {
        self.bottom_em - self.top_em
    }
}

/// Resolution the templates are drawn at, in pixels per em.
const DRAW_PX_PER_EM: f32 = 96.0;

/// Every template, built once.
pub static TEMPLATES: LazyLock<Vec<Template>> = LazyLock::new(build);

/// The ink of the glyph `glyph` of the TeX font `font` (`cmmi10`) at
/// `px_per_em` pixels per em, placed against its origin on the baseline (y
/// downwards), and its advance in pixels.
#[cfg(test)]
pub fn draw(font: &str, glyph: &str, px_per_em: f32) -> Option<(Mask, f32)> {
    let tex_font = TEX_FONTS.iter().find(|tex_font| tex_font.name == font)?;
    let loaded = Font::new(tex_font.data, 0)?;
    let id = loaded.glyph_by_name(glyph)?;
    let mut pen = Polygons::default();
    let scaler = ScalerFont::unhinted(loaded.clone(), Some(px_per_em));
    scaler.outline(id)?.draw(DrawMemory::Scratch, &mut pen).ok()?;
    let advance = loaded.glyph_metrics().h_advance(id) * px_per_em / f32::from(loaded.units_per_em().max(1));
    Some((pen.fill()?, advance))
}

/// Draws the templates of the glyphs of the TeX fonts that formulas use.
fn build() -> Vec<Template> {
    let mut out = Vec::new();
    for tex_font in TEX_FONTS {
        let Some(loaded) = Font::new(tex_font.data, 0) else {
            continue;
        };
        let scale = DRAW_PX_PER_EM / f32::from(loaded.units_per_em().max(1));
        let scaler = ScalerFont::unhinted(loaded.clone(), Some(DRAW_PX_PER_EM));
        for (id, name) in loaded.glyph_names() {
            let Some((text, kind)) = tex::glyph(tex_font.role, name.as_str()) else {
                continue;
            };
            let mut pen = Polygons::default();
            let Some(outline) = scaler.outline(id) else { continue };
            if outline.draw(DrawMemory::Scratch, &mut pen).is_err() {
                continue;
            }
            let Some(mask) = pen.fill() else { continue };
            let advance = loaded.glyph_metrics().h_advance(id) * scale;
            out.push(template(
                mask,
                advance,
                text,
                tex::output_font(tex_font.role),
                kind,
                tex_font.level,
            ));
        }
    }
    out
}

/// The template of a glyph drawn as `mask` at [`DRAW_PX_PER_EM`], `advance`
/// pixels wide.
fn template(mask: Mask, advance: f32, text: String, font: u16, kind: Kind, level: f32) -> Template {
    let (top, bottom) = (mask.origin_y, mask.origin_y + mask.height as f32);
    let before = mask.origin_x;
    let after = advance - before - mask.width as f32;
    Template {
        text,
        font,
        kind,
        top_em: top / DRAW_PX_PER_EM,
        bottom_em: bottom / DRAW_PX_PER_EM,
        parts: mask.parts(),
        level,
        ink_em2: mask.ink.iter().sum::<f32>() / (DRAW_PX_PER_EM * DRAW_PX_PER_EM),
        before_em: before / DRAW_PX_PER_EM,
        after_em: after / DRAW_PX_PER_EM,
        shape: Shape::of(&mask),
    }
}

/// The outline of a glyph as closed polygons, in pixels (y downwards).
#[derive(Default)]
struct Polygons {
    /// Finished polygons.
    done: Vec<Vec<(f32, f32)>>,
    /// The polygon being drawn.
    current: Vec<(f32, f32)>,
}

/// Straight segments a curve is cut into.
const CURVE_STEPS: usize = 8;

impl OutlinePen for Polygons {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.current.push((x, -y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push((x, -y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (x0, y0) = self.current.last().copied().unwrap_or((x, -y));
        for step in 1..=CURVE_STEPS {
            let to = step as f32 / CURVE_STEPS as f32;
            let from = 1.0 - to;
            self.current.push((
                from * from * x0 + 2.0 * from * to * cx0 + to * to * x,
                from * from * y0 + 2.0 * from * to * -cy0 + to * to * -y,
            ));
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (x0, y0) = self.current.last().copied().unwrap_or((x, -y));
        for step in 1..=CURVE_STEPS {
            let to = step as f32 / CURVE_STEPS as f32;
            let from = 1.0 - to;
            let weights = [
                from * from * from,
                3.0 * from * from * to,
                3.0 * from * to * to,
                to * to * to,
            ];
            self.current.push((
                weights[0] * x0 + weights[1] * cx0 + weights[2] * cx1 + weights[3] * x,
                weights[0] * y0 + weights[1] * -cy0 + weights[2] * -cy1 + weights[3] * -y,
            ));
        }
    }

    fn close(&mut self) {
        if self.current.len() > 2 {
            self.done.push(std::mem::take(&mut self.current));
        }
        self.current.clear();
    }
}

impl Polygons {
    /// The ink of the polygons (non-zero winding), sampled at pixel
    /// centres; `None` for an empty glyph.
    fn fill(mut self) -> Option<Mask> {
        self.close();
        let points = self.done.iter().flatten();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for &(x, y) in points {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        if !(x1 > x0 && y1 > y0) {
            return None;
        }
        let (left, top) = (x0.floor(), y0.floor());
        let width = (x1.ceil() - left) as usize;
        let height = (y1.ceil() - top) as usize;
        let mut ink = vec![0.0f32; width * height];
        for row in 0..height {
            let yc = top + row as f32 + 0.5;
            let mut crossings: Vec<(f32, i32)> = Vec::new();
            for polygon in &self.done {
                for (k, &(ax, ay)) in polygon.iter().enumerate() {
                    let (bx, by) = polygon[(k + 1) % polygon.len()];
                    if (ay <= yc) != (by <= yc) {
                        let x = ax + (yc - ay) / (by - ay) * (bx - ax);
                        crossings.push((x, if by > ay { 1 } else { -1 }));
                    }
                }
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0;
            for pair in crossings.windows(2) {
                winding += pair[0].1;
                if winding != 0 {
                    let from = (pair[0].0 - left - 0.5).ceil().max(0.0) as usize;
                    let to = ((pair[1].0 - left - 0.5).floor() + 1.0).clamp(0.0, width as f32) as usize;
                    for cell in &mut ink[row * width + from.min(width)..row * width + to] {
                        *cell = 1.0;
                    }
                }
            }
        }
        Mask::new(ink, width, height, left, top).cropped()
    }
}
