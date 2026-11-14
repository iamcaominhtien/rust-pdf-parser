//! Formulas from pictures: the ink of a picture of a formula (white
//! background, black glyphs, as LaTeX renders them) recognised glyph by
//! glyph against the TeX fonts, then rebuilt like a formula of a PDF.

mod recognize;
mod shape;
mod templates;
mod tex;

pub use recognize::{RasterConfig, Reading, read};

use crate::analysis::math::formula_from_glyphs;
use crate::backend::FontInfo;
use crate::model::MathNode;
use std::sync::LazyLock;

/// The font table the glyphs read from pictures refer to (see
/// [`templates::font`]), named after the TeX fonts of each kind of glyph.
static FONTS: LazyLock<Vec<FontInfo>> = LazyLock::new(|| {
    [
        "CMMI10", "CMR10", "CMSY10", "CMEX10", "MSBM10", "CMBX10", "CMMIB10", "EUFM10",
    ]
    .iter()
    .map(|name| FontInfo::from_name(name, 0, 0.0, 0.0))
    .collect()
});

/// The formula pictured in `image` (any colour picture: transparent parts
/// count as white), or `None` when it cannot be read with confidence.
pub fn formula(image: &image::DynamicImage, cfg: &RasterConfig) -> Option<MathNode> {
    let rgba = image.to_rgba8();
    let gray: Vec<u8> = rgba
        .pixels()
        .map(|pixel| {
            let [red, green, blue, alpha] = pixel.0.map(u32::from);
            let luma = (299 * red + 587 * green + 114 * blue) / 1000;
            u8::try_from((luma * alpha + 255 * (255 - alpha)) / 255).unwrap_or(u8::MAX)
        })
        .collect();
    let reading = read(&gray, rgba.width() as usize, cfg)?;
    formula_from_glyphs(reading.glyphs, &reading.rules, &FONTS)
}

#[cfg(test)]
#[allow(clippy::min_ident_chars, clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;
    use crate::render::math_html;
    use image::{DynamicImage, GrayImage, Luma};

    /// Pixels per em of the test pictures.
    const PX: f32 = 40.0;

    /// Supersampling of the test pictures (drawn this many times larger,
    /// then averaged: anti-aliased, like real pictures).
    const SUPER: usize = 4;

    /// A picture of glyphs of TeX fonts (font, glyph name), each placed at
    /// (x, baseline) in ems with a scale, and of bars (x0, x1, y in ems).
    fn picture(glyphs: &[(&str, &str, f32, f32, f32)], bars: &[(f32, f32, f32)]) -> DynamicImage {
        let (width, height) = (5 * PX as usize, 3 * PX as usize);
        let big = PX * SUPER as f32;
        let mut ink = vec![0u32; width * SUPER * height * SUPER];
        let mut put = |x: usize, y: usize| {
            if x < width * SUPER && y < height * SUPER {
                ink[y * width * SUPER + x] = 1;
            }
        };
        for &(font, glyph, x, baseline, scale) in glyphs {
            let (mask, _) = templates::draw(font, glyph, big * scale).expect("glyph");
            for row in 0..mask.height {
                for col in 0..mask.width {
                    if mask.cover(col, row) > 0.0 {
                        put(
                            (x * big + mask.origin_x) as usize + col,
                            (baseline * big + mask.origin_y) as usize + row,
                        );
                    }
                }
            }
        }
        for &(x0, x1, y) in bars {
            for x in (x0 * big) as usize..(x1 * big) as usize {
                for dy in 0..(0.05 * big) as usize {
                    put(x, (y * big) as usize + dy);
                }
            }
        }
        let img = GrayImage::from_fn(width as u32, height as u32, |x, y| {
            let mut sum = 0;
            for dy in 0..SUPER {
                for dx in 0..SUPER {
                    sum += ink[(y as usize * SUPER + dy) * width * SUPER + x as usize * SUPER + dx];
                }
            }
            Luma([255 - (255 * sum / (SUPER * SUPER) as u32) as u8])
        });
        DynamicImage::ImageLuma8(img)
    }

    fn read_math(img: &DynamicImage) -> String {
        let node = formula(img, &RasterConfig::default()).expect("a formula");
        math_html(&node, "").replace(" stretchy=\"false\"", "")
    }

    #[test]
    fn the_tex_fonts_give_templates() {
        assert!(templates::TEMPLATES.len() > 1000);
    }

    #[test]
    fn letters_and_operators() {
        let img = picture(
            &[
                ("cmmi10", "x", 0.5, 1.5, 1.0),
                ("cmr10", "plus", 1.3, 1.5, 1.0),
                ("cmmi10", "y", 2.4, 1.5, 1.0),
            ],
            &[],
        );
        assert!(read_math(&img).contains("<mi>x</mi><mo>+</mo><mi>y</mi>"));
    }

    #[test]
    fn a_bar_with_letters_above_and_below_is_a_fraction() {
        let img = picture(
            &[("cmmi10", "a", 0.7, 1.1, 1.0), ("cmmi10", "b", 0.75, 2.1, 1.0)],
            &[(0.5, 1.5, 1.25)],
        );
        assert!(read_math(&img).contains("<mfrac><mi>a</mi><mi>b</mi></mfrac>"));
    }

    #[test]
    fn a_small_raised_digit_is_a_superscript() {
        let img = picture(&[("cmmi10", "x", 0.5, 1.5, 1.0), ("cmr7", "two", 1.1, 1.15, 0.7)], &[]);
        assert!(read_math(&img).contains("<msup><mi>x</mi><mn>2</mn></msup>"));
    }

    #[test]
    fn two_bars_are_an_equals_sign() {
        let img = picture(
            &[
                ("cmmi10", "a", 0.5, 1.5, 1.0),
                ("cmr10", "equal", 1.3, 1.5, 1.0),
                ("cmr10", "one", 2.3, 1.5, 1.0),
            ],
            &[],
        );
        assert!(read_math(&img).contains("<mi>a</mi><mo>=</mo><mn>1</mn>"));
    }
}
