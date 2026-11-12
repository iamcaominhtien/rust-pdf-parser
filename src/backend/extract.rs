//! Runs the PDF interpreter over a page and records everything the layout
//! analysis needs: positioned glyphs with their fonts, image placements and
//! the bounding boxes of vector drawing operations.

use super::fonts::{FontRegistry, UNKNOWN_FONT};
use crate::common::geom::Rect;
use hayro_interpret::font::{Glyph as PdfGlyph, GlyphRun};
use hayro_interpret::hayro_cmap::BfString;
use hayro_interpret::util::TransformExt;
use hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache,
    InterpreterSettings, Paint, SoftMask, interpret_page,
};
use hayro_syntax::page::Page;
use kurbo::{Affine, BezPath, Point, Shape};

/// Text direction of a glyph, quantised to quarter turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Regular left-to-right text.
    Horizontal,
    /// Any rotated text (axis labels, the arXiv margin stamp, ...).
    Rotated,
}

#[derive(Debug, Clone)]
pub struct Glyph {
    pub text: String,
    /// Origin on the baseline.
    pub x: f32,
    pub y: f32,
    /// Advance along the baseline.
    pub adv: f32,
    pub size: f32,
    pub font: u16,
    pub dir: Dir,
    /// Rotation in degrees (0 for horizontal text).
    pub angle: f32,
    pub invisible: bool,
    pub color: [u8; 3],
    /// Bounding box of the glyph's actual outline.
    pub ink: Rect,
}

impl Glyph {
    /// Nominal box of the glyph (em box, not the ink).
    pub fn bbox(&self) -> Rect {
        match self.dir {
            Dir::Horizontal => Rect::new(
                self.x,
                self.y - 0.75 * self.size,
                self.x + self.adv.max(0.0),
                self.y + 0.22 * self.size,
            ),
            Dir::Rotated => {
                let r = self.size.max(self.adv.abs());
                Rect::new(self.x - r, self.y - r, self.x + r, self.y + r)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ImageBox {
    pub bbox: Rect,
    pub px_width: u32,
    pub px_height: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct PathBox {
    pub bbox: Rect,
    pub stroked: bool,
    pub filled: bool,
    pub color: [u8; 4],
    pub line_width: f32,
    /// Number of path elements; 5 or fewer usually means a line or a box.
    pub elements: u16,
}

impl PathBox {
    pub fn is_white(&self) -> bool {
        self.color[0] > 250 && self.color[1] > 250 && self.color[2] > 250
    }

    /// A thin horizontal line (table rule, footnote rule, fraction bar).
    pub fn is_hrule(&self) -> bool {
        self.bbox.height() <= 2.5 && self.bbox.width() >= 4.0 * self.bbox.height().max(0.5)
    }

    pub fn is_vrule(&self) -> bool {
        self.bbox.width() <= 2.5 && self.bbox.height() >= 4.0 * self.bbox.width().max(0.5)
    }
}

#[derive(Debug, Default)]
pub struct RawPage {
    pub index: usize,
    pub width: f32,
    pub height: f32,
    pub glyphs: Vec<Glyph>,
    pub images: Vec<ImageBox>,
    pub paths: Vec<PathBox>,
}

struct Extractor<'r, 'a> {
    fonts: &'r mut FontRegistry<'a>,
    page: RawPage,
    page_rect: kurbo::Rect,
    clips: Vec<kurbo::Rect>,
}

impl Extractor<'_, '_> {
    fn clip(&self) -> kurbo::Rect {
        self.clips.last().copied().unwrap_or(self.page_rect)
    }

    fn visible(&self, bbox: kurbo::Rect) -> Option<Rect> {
        let c = self.clip();
        let r = bbox.intersect(c);
        // `intersect` of disjoint rects yields a zero-area rect, lines also
        // have zero area, so test for overlap explicitly.
        if bbox.x0 > c.x1 || bbox.x1 < c.x0 || bbox.y0 > c.y1 || bbox.y1 < c.y0 {
            return None;
        }
        Some(Rect::new(r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32))
    }
}

fn paint_color(paint: &Paint<'_>) -> [u8; 4] {
    match paint {
        Paint::Color(c) => c.to_rgba().to_rgba8(),
        Paint::Pattern(_) => [128, 128, 128, 255],
    }
}

fn normalize_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2009}' | '\u{200A}' | '\u{202F}' => out.push(' '),
            '\u{00AD}' => out.push('-'),
            '\u{2010}' | '\u{2011}' => out.push('-'),
            '\u{0}'..='\u{1F}' | '\u{7F}' | '\u{200B}'..='\u{200F}' | '\u{FEFF}' => {}
            c => out.push(c),
        }
    }
}

impl<'a> Device<'a> for Extractor<'_, 'a> {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, draw_mode: &DrawMode) {
        let (stroked, filled, lw) = match draw_mode {
            DrawMode::Fill(_) => (false, true, 0.0),
            DrawMode::Stroke(s) => (true, false, s.line_width),
            DrawMode::FillAndStroke(_, s) => (true, true, s.line_width),
            DrawMode::Invisible => return,
        };
        let color = paint_color(&props.paint);
        if color[3] == 0 {
            return;
        }
        let [a, b, c, d, _, _] = props.transform.as_coeffs();
        let scale = ((a * d - b * c).abs()).sqrt();
        let lw_dev = lw as f64 * scale;
        let mut bbox = props.transform.transform_rect_bbox(path.bounding_box());
        if stroked {
            bbox = bbox.inflate(lw_dev * 0.5, lw_dev * 0.5);
        }
        let Some(bbox) = self.visible(bbox) else {
            return;
        };
        self.page.paths.push(PathBox {
            bbox,
            stroked,
            filled,
            color,
            line_width: lw_dev as f32,
            elements: path.elements().len().min(u16::MAX as usize) as u16,
        });
    }

    fn push_clip_path(&mut self, clip_path: &ClipPath) {
        let bbox = clip_path.path.bounding_box();
        let cur = self.clip();
        let mut r = bbox.intersect(cur);
        if bbox.x0 > cur.x1 || bbox.x1 < cur.x0 || bbox.y0 > cur.y1 || bbox.y1 < cur.y0 {
            // Empty clip: nothing drawn inside is visible.
            r = kurbo::Rect::new(-1e6, -1e6, -1e6 + 1e-3, -1e6 + 1e-3);
        }
        self.clips.push(r);
    }

    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}

    fn draw_glyph_run(&mut self, glyph_run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, draw_mode: &DrawMode) {
        let invisible = matches!(draw_mode, DrawMode::Invisible);
        let color = paint_color(&props.paint);
        let clip = self.clip();
        for glyph in glyph_run.glyphs() {
            let t: Affine = props.transform * glyph.transform();
            let origin = t * Point::ZERO;
            if origin.x < clip.x0 - 2.0
                || origin.x > clip.x1 + 2.0
                || origin.y < clip.y0 - 2.0
                || origin.y > clip.y1 + 2.0
            {
                continue;
            }
            let [a, b, c, d, _, _] = t.as_coeffs();
            // Glyph space is 1000 units per em.
            let size = (c.hypot(d) * 1000.0) as f32;
            let xlen = a.hypot(b) * 1000.0;
            if size < 0.5 || !size.is_finite() {
                continue;
            }
            let angle = (b.atan2(a)).to_degrees() as f32;
            let dir = if angle.abs() < 3.0 {
                Dir::Horizontal
            } else {
                Dir::Rotated
            };

            let (unicode, advance, font, outline_box) = match &**glyph {
                PdfGlyph::Outline(g) => (
                    g.as_unicode(),
                    g.advance_width(),
                    self.fonts.identify(g),
                    self.fonts.outline_bbox(g),
                ),
                PdfGlyph::Type3(g) => (g.as_unicode(), g.advance_width(), UNKNOWN_FONT, None),
            };
            let mut text = String::new();
            match unicode {
                Some(BfString::Char(c)) => {
                    let mut buf = [0u8; 4];
                    normalize_text(c.encode_utf8(&mut buf), &mut text);
                }
                Some(BfString::String(s)) => normalize_text(&s, &mut text),
                None => text.push('\u{FFFD}'),
            }
            if text.is_empty() {
                continue;
            }
            if self.fonts.infos[font as usize].math_pi {
                text = text
                    .chars()
                    .map(|c| match c {
                        '¼' => '=',
                        'þ' => '+',
                        'ð' => '(',
                        'Þ' => ')',
                        '½' => '[',
                        c => c,
                    })
                    .collect();
            }
            // Use the horizontal extent of the transform so that horizontally
            // scaled text keeps correct advances.
            let adv = advance.unwrap_or(500.0) as f64 / 1000.0 * xlen;
            let ink = match outline_box {
                Some(b) => {
                    let r = t.transform_rect_bbox(b);
                    Rect::new(r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32)
                }
                None => Rect::new(
                    origin.x as f32,
                    origin.y as f32 - 0.7 * size,
                    (origin.x + adv) as f32,
                    origin.y as f32 + 0.2 * size,
                ),
            };
            self.page.glyphs.push(Glyph {
                text,
                x: origin.x as f32,
                y: origin.y as f32,
                adv: adv as f32,
                size,
                font,
                dir,
                angle,
                invisible,
                color: [color[0], color[1], color[2]],
                ink,
            });
        }
    }

    fn draw_image(&mut self, image: Image<'a, '_>, props: ImageDrawProps<'a>) {
        let (w, h) = (image.width(), image.height());
        let rect = kurbo::Rect::new(0.0, 0.0, w as f64, h as f64);
        let bbox = props.transform.transform_rect_bbox(rect);
        if let Some(bbox) = self.visible(bbox)
            && bbox.width() > 0.5
            && bbox.height() > 0.5
        {
            self.page.images.push(ImageBox {
                bbox,
                px_width: w,
                px_height: h,
            });
        }
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn pop_transparency_group(&mut self) {}
}

/// Interprets one page and returns its raw content.
pub fn extract_page<'a>(
    page: &'a Page<'a>,
    index: usize,
    fonts: &mut FontRegistry<'a>,
    cache: &InterpreterCache<'a>,
    settings: &InterpreterSettings,
) -> RawPage {
    let (width, height) = page.render_dimensions();
    let page_rect = kurbo::Rect::new(0.0, 0.0, width as f64, height as f64);
    let mut ctx = Context::new(
        page.initial_transform(true).to_kurbo(),
        page_rect,
        cache,
        page.xref(),
        settings.clone(),
    );
    fonts.begin_page(page);
    let mut device = Extractor {
        fonts,
        page: RawPage {
            index,
            width,
            height,
            ..Default::default()
        },
        page_rect,
        clips: Vec::new(),
    };
    interpret_page(page, &mut ctx, &mut device);
    device.page
}
