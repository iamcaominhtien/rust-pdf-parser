//! The glyphs a picture of a formula is compared with: the symbols of
//! Latin Modern Math (the font of LaTeX formulas), drawn once from its
//! outlines.

use super::shape::{Mask, Shape};
use super::woff2;
use crate::render::LATIN_MODERN_MATH;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
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

/// The math font, unpacked once.
static FONT_DATA: LazyLock<Option<Vec<u8>>> = LazyLock::new(|| woff2::decode(LATIN_MODERN_MATH));

/// The ink of the character `drawn` of the math font at `px_per_em` pixels
/// per em, placed against its origin on the baseline (y downwards), and its
/// advance in pixels.
pub fn draw(drawn: char, px_per_em: f32) -> Option<(Mask, f32)> {
    let font_ref = FontRef::new(FONT_DATA.as_deref()?).ok()?;
    let id = font_ref.charmap().map(drawn)?;
    let glyph = font_ref.outline_glyphs().get(id)?;
    let mut pen = Polygons::default();
    glyph
        .draw(
            DrawSettings::unhinted(Size::new(px_per_em), LocationRef::default()),
            &mut pen,
        )
        .ok()?;
    let mask = pen.fill()?;
    let metrics = font_ref.glyph_metrics(Size::new(px_per_em), LocationRef::default());
    let advance = metrics.advance_width(id).unwrap_or(mask.width as f32);
    Some((mask, advance))
}

/// Draws the templates of [`symbols`].
fn build() -> Vec<Template> {
    symbols()
        .into_iter()
        .filter_map(|(drawn, text, font, kind)| {
            let (mask, advance) = draw(drawn, DRAW_PX_PER_EM)?;
            let (top, bottom) = (mask.origin_y, mask.origin_y + mask.height as f32);
            let before = mask.origin_x;
            let after = advance - before - mask.width as f32;
            Some(Template {
                text,
                font,
                kind,
                top_em: top / DRAW_PX_PER_EM,
                bottom_em: bottom / DRAW_PX_PER_EM,
                parts: mask.parts(),
                ink_em2: mask.ink.iter().sum::<f32>() / (DRAW_PX_PER_EM * DRAW_PX_PER_EM),
                before_em: before / DRAW_PX_PER_EM,
                after_em: after / DRAW_PX_PER_EM,
                shape: Shape::of(&mask),
            })
        })
        .collect()
}

/// The symbols: the character drawn, the text it stands for, its font and
/// kind.
fn symbols() -> Vec<(char, String, u16, Kind)> {
    let mut out = letters();
    let mut add = |drawn: char, text: &str, font: u16, kind: Kind| out.push((drawn, text.to_string(), font, kind));
    for symbol in "+=()[],.;:!?/%'".chars() {
        let kind = if "()[]".contains(symbol) {
            Kind::Delimiter
        } else {
            Kind::Symbol
        };
        add(symbol, &symbol.to_string(), font::ROMAN, kind);
    }
    for symbol in "−×·÷±∓≤≥≠≈∼≃≅≡∝∈∉∋⊂⊃⊆⊇∪∩∧∨¬→←↔⇒⇐⇔↦∞∂∇∀∃∅′∗∘⊗⊕⊙⊥⊤≪≫≺≻⪯⪰↑↓≲≳∖⋆†⋯…⋮⋱<>∣".chars()
    {
        add(symbol, &symbol.to_string(), font::SYMBOL, Kind::Symbol);
    }
    for symbol in "{}|‖⟨⟩⌊⌋⌈⌉√".chars() {
        add(symbol, &symbol.to_string(), font::SYMBOL, Kind::Delimiter);
    }
    for symbol in "∑∏∐∫∮⋃⋂⨁⨂".chars() {
        add(symbol, &symbol.to_string(), font::EXTENSION, Kind::BigOp);
    }
    for (drawn, text) in [
        ('ˆ', "ˆ"),
        ('˜', "˜"),
        ('¯', "¯"),
        ('˙', "˙"),
        ('¨', "¨"),
        ('ˇ', "ˇ"),
        ('˘', "˘"),
    ] {
        add(drawn, text, font::ROMAN, Kind::Accent);
    }
    out
}

/// The letters and digits of every alphabet: the character drawn, the
/// text it stands for, its font and kind.
fn letters() -> Vec<(char, String, u16, Kind)> {
    let mut out = Vec::new();
    let mut add = |drawn: char, text: &str, font: u16, kind: Kind| out.push((drawn, text.to_string(), font, kind));
    for (offset, letter) in ('a'..='z').enumerate() {
        let italic = if letter == 'h' { 'ℎ' } else { shifted('𝑎', offset) };
        add(italic, &letter.to_string(), font::ITALIC, Kind::Letter);
        add(letter, &letter.to_string(), font::ROMAN, Kind::Letter);
        add(shifted('𝐚', offset), &letter.to_string(), font::BOLD, Kind::Letter);
        add(
            shifted('𝒂', offset),
            &letter.to_string(),
            font::BOLD_ITALIC,
            Kind::Letter,
        );
        add(fraktur(letter), &letter.to_string(), font::FRAKTUR, Kind::Letter);
    }
    for (offset, letter) in ('A'..='Z').enumerate() {
        add(shifted('𝐴', offset), &letter.to_string(), font::ITALIC, Kind::Letter);
        add(letter, &letter.to_string(), font::ROMAN, Kind::Letter);
        add(shifted('𝐀', offset), &letter.to_string(), font::BOLD, Kind::Letter);
        add(
            shifted('𝑨', offset),
            &letter.to_string(),
            font::BOLD_ITALIC,
            Kind::Letter,
        );
        add(script(letter), &letter.to_string(), font::SYMBOL, Kind::Letter);
        add(fraktur(letter), &letter.to_string(), font::FRAKTUR, Kind::Letter);
    }
    for (offset, digit) in ('0'..='9').enumerate() {
        add(digit, &digit.to_string(), font::ROMAN, Kind::Letter);
        add(shifted('𝟎', offset), &digit.to_string(), font::BOLD, Kind::Letter);
    }
    for (offset, greek) in ('α'..='ω').enumerate() {
        if greek != 'ς' {
            add(shifted('𝛼', offset), &greek.to_string(), font::ITALIC, Kind::Letter);
        }
    }
    for (drawn, text) in [
        ('𝜕', "∂"),
        ('𝜖', "ϵ"),
        ('𝜗', "ϑ"),
        ('𝜙', "ϕ"),
        ('𝜚', "ϱ"),
        ('𝜛', "ϖ"),
        ('ℓ', "ℓ"),
    ] {
        add(drawn, text, font::ITALIC, Kind::Letter);
    }
    for greek in "ΓΔΘΛΞΠΣΥΦΨΩ".chars() {
        add(greek, &greek.to_string(), font::ROMAN, Kind::Letter);
    }
    for (drawn, text) in [
        ('ℝ', "R"),
        ('ℕ', "N"),
        ('ℤ', "Z"),
        ('ℚ', "Q"),
        ('ℂ', "C"),
        ('𝔼', "E"),
        ('ℙ', "P"),
    ] {
        add(drawn, text, font::DOUBLE_STRUCK, Kind::Letter);
    }
    out
}

/// The character `offset` places after `first` (in an alphabet of the
/// mathematical alphanumerics block).
fn shifted(first: char, offset: usize) -> char {
    u32::try_from(offset)
        .ok()
        .and_then(|offset| char::from_u32(u32::from(first) + offset))
        .unwrap_or(first)
}

/// The Fraktur form of `letter`.
fn fraktur(letter: char) -> char {
    match letter {
        'C' => 'ℭ',
        'H' => 'ℌ',
        'I' => 'ℑ',
        'R' => 'ℜ',
        'Z' => 'ℨ',
        'a'..='z' => shifted('𝔞', usize::from(u8::try_from(letter).unwrap_or(b'a') - b'a')),
        _ => shifted('𝔄', usize::from(u8::try_from(letter).unwrap_or(b'A') - b'A')),
    }
}

/// The script capital of `letter`.
fn script(letter: char) -> char {
    match letter {
        'B' => 'ℬ',
        'E' => 'ℰ',
        'F' => 'ℱ',
        'H' => 'ℋ',
        'I' => 'ℐ',
        'L' => 'ℒ',
        'M' => 'ℳ',
        'R' => 'ℛ',
        _ => shifted('𝒜', usize::from(u8::try_from(letter).unwrap_or(b'A') - b'A')),
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
