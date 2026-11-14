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

/// Height of a glyph's nominal box above the baseline, in multiples of the
/// font size (roughly the ascender height of Latin text fonts).
const EM_BOX_ASCENT_EM: f32 = 0.75;
/// Depth of a glyph's nominal box below the baseline, in multiples of the
/// font size (roughly the descender depth of Latin text fonts).
const EM_BOX_DESCENT_EM: f32 = 0.22;
/// A colour whose red, green and blue channels all exceed this value counts
/// as white (white boxes are often used to mask parts of a figure).
const WHITE_CHANNEL_MIN: u8 = 250;
/// Maximum thickness of a path that can be a rule, in points.
const RULE_MAX_THICKNESS_PT: f32 = 2.5;
/// Minimum length of a rule, in multiples of its thickness.
const RULE_MIN_ASPECT: f32 = 4.0;
/// Thickness assumed for the aspect test of rules thinner than this (in
/// points), so that hairlines of zero width still need some length.
const RULE_MIN_THICKNESS_PT: f32 = 0.5;
/// Colour recorded for pattern (gradient, tiling) paints, which have no
/// single colour: an opaque mid grey, so they are neither white nor
/// transparent.
const PATTERN_COLOR: [u8; 4] = [128, 128, 128, 255];
/// Clip rectangle standing for an empty clip: a tiny box far outside any
/// page, so that nothing drawn inside it is visible.
const EMPTY_CLIP: kurbo::Rect = kurbo::Rect::new(-1e6, -1e6, -1e6 + 1e-3, -1e6 + 1e-3);
/// Glyphs whose origin lies up to this far outside the clip rectangle are
/// still kept, in points (origins on the clip edge are common and harmless).
const GLYPH_CLIP_MARGIN_PT: f64 = 2.0;
/// Units per em of glyph space, the PDF convention for font metrics.
const GLYPH_UNITS_PER_EM: f64 = 1000.0;
/// Glyphs smaller than this (in points) are dropped: they are invisible
/// text or degenerate transforms.
const MIN_GLYPH_SIZE_PT: f32 = 0.5;
/// Glyphs whose baseline deviates less than this from horizontal (in
/// degrees) count as horizontal text.
const HORIZONTAL_MAX_ANGLE_DEG: f32 = 3.0;
/// Advance assumed for glyphs without a width, in glyph units (half an em).
const DEFAULT_ADVANCE_UNITS: f32 = 500.0;
/// Height of the ink box above the baseline assumed for glyphs without an
/// outline, in multiples of the font size.
const FALLBACK_INK_ASCENT_EM: f32 = 0.7;
/// Depth of the ink box below the baseline assumed for glyphs without an
/// outline, in multiples of the font size.
const FALLBACK_INK_DESCENT_EM: f32 = 0.2;
/// A glyph mapped to `=` whose outline is this many times taller than wide
/// is a slash: an equals sign is always wider than tall. Some publishers'
/// fonts (APS) keep the TeX math italic layout, where the code of `=` holds
/// the slash, but declare a text encoding.
const SLASH_MIN_ASPECT: f64 = 1.5;
/// Images whose visible part is not wider and taller than this (in points)
/// are ignored (invisible placeholder images, masks of single pixels).
const MIN_IMAGE_SIDE_PT: f32 = 0.5;
/// A stencil mask with at most this many pixels, stretched over a long thin
/// box, is a rule: some producers draw table lines as one-pixel masks.
const STENCIL_RULE_MAX_PIXELS: u32 = 1;
/// Number of path elements recorded for a rule drawn as a stencil mask (the
/// four sides of a rectangle and its closing).
const STENCIL_RULE_ELEMENTS: u16 = 5;

/// A picture of a single pixel stretched into a thin bar is a rule drawn
/// the way some typesetters draw table rules (ATLAS): it is recorded in
/// [`RawPage::pixel_rules`] as a filled path. This is the colour assumed for
/// it (black: the pixel is not read).
const PIXEL_RULE_COLOR: [u8; 4] = [0, 0, 0, 255];
/// A single-pixel picture is a rule when its thin side is at most this
/// (in points).
const PIXEL_RULE_MAX_THICKNESS_PT: f32 = 2.0;

/// Text direction of a glyph, quantised to quarter turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Regular left-to-right text.
    Horizontal,
    /// Any rotated text (axis labels, the arXiv margin stamp, ...).
    Rotated,
}

/// A glyph as drawn on the page, in page coordinates (points, y down).
#[derive(Debug, Clone)]
pub struct Glyph {
    /// The text the glyph stands for, normalised (ligatures expanded,
    /// special spaces and hyphens replaced, control characters removed).
    /// U+FFFD when the PDF gives no Unicode mapping.
    pub text: String,
    /// Origin on the baseline.
    pub x: f32,
    /// Vertical coordinate of the origin (see [`Glyph::x`]).
    pub y: f32,
    /// Advance along the baseline.
    pub adv: f32,
    /// Font size in points (the vertical scale of the glyph transform).
    pub size: f32,
    /// Index into the document's font table ([`UNKNOWN_FONT`] for Type 3
    /// glyphs and glyphs whose font could not be identified).
    pub font: u16,
    /// Direction of the baseline.
    pub dir: Dir,
    /// Rotation in degrees (0 for horizontal text).
    pub angle: f32,
    /// Drawn in the invisible text rendering mode (OCR layers of scans).
    pub invisible: bool,
    /// Fill colour as RGB.
    pub color: [u8; 3],
    /// Bounding box of the glyph's actual outline.
    pub ink: Rect,
}

impl Glyph {
    /// Nominal box of the glyph (em box, not the ink).
    ///
    /// For rotated glyphs this is a square around the origin that contains
    /// the glyph whatever its direction.
    pub fn bbox(&self) -> Rect {
        match self.dir {
            Dir::Horizontal => Rect::new(
                self.x,
                self.y - EM_BOX_ASCENT_EM * self.size,
                self.x + self.adv.max(0.0),
                self.y + EM_BOX_DESCENT_EM * self.size,
            ),
            Dir::Rotated => {
                let radius = self.size.max(self.adv.abs());
                Rect::new(self.x - radius, self.y - radius, self.x + radius, self.y + radius)
            }
        }
    }
}

/// Where a raster image is placed on the page.
#[derive(Debug, Clone, Copy)]
pub struct ImageBox {
    /// Visible part of the image on the page.
    pub bbox: Rect,
    /// Width of the image in pixels.
    pub px_width: u32,
    /// Height of the image in pixels.
    pub px_height: u32,
}

/// A filled or stroked vector path, reduced to its bounding box.
#[derive(Debug, Clone, Copy)]
pub struct PathBox {
    /// Visible part of the path's bounding box (including half the stroke
    /// width for stroked paths).
    pub bbox: Rect,
    /// The path is stroked.
    pub stroked: bool,
    /// The path is filled.
    pub filled: bool,
    /// Paint colour as RGBA.
    pub color: [u8; 4],
    /// Stroke width on the page in points (0 for fills).
    pub line_width: f32,
    /// Number of path elements; 5 or fewer usually means a line or a box.
    pub elements: u16,
}

impl PathBox {
    /// The path is painted white (or nearly so), e.g. a mask over part of a
    /// figure.
    pub fn is_white(&self) -> bool {
        self.color[0] > WHITE_CHANNEL_MIN && self.color[1] > WHITE_CHANNEL_MIN && self.color[2] > WHITE_CHANNEL_MIN
    }

    /// A thin horizontal line (table rule, footnote rule, fraction bar).
    pub fn is_hrule(&self) -> bool {
        self.bbox.height() <= RULE_MAX_THICKNESS_PT
            && self.bbox.width() >= RULE_MIN_ASPECT * self.bbox.height().max(RULE_MIN_THICKNESS_PT)
    }

    /// A thin vertical line (table rule, column separator).
    pub fn is_vrule(&self) -> bool {
        self.bbox.width() <= RULE_MAX_THICKNESS_PT
            && self.bbox.height() >= RULE_MIN_ASPECT * self.bbox.width().max(RULE_MIN_THICKNESS_PT)
    }
}

/// Everything recorded on one page.
#[derive(Debug, Default)]
pub struct RawPage {
    /// Zero-based page index.
    pub index: usize,
    /// Page width in points (after the page's rotation).
    pub width: f32,
    /// Page height in points (after the page's rotation).
    pub height: f32,
    /// Glyphs in drawing order.
    pub glyphs: Vec<Glyph>,
    /// Raster images in drawing order.
    pub images: Vec<ImageBox>,
    /// Vector paths in drawing order.
    pub paths: Vec<PathBox>,
    /// Bars drawn as a stretched picture of one pixel (see
    /// [`PIXEL_RULE_MAX_THICKNESS_PT`]). They are kept apart from `paths`:
    /// only table rules are read from them, the detection of figures and
    /// captions does not see them.
    pub pixel_rules: Vec<PathBox>,
}

/// The interpreter's output device: receives drawing operations and
/// records them into a [`RawPage`].
struct Extractor<'r, 'a> {
    /// Fonts of the document, used to identify the font of each glyph.
    fonts: &'r mut FontRegistry<'a>,
    /// The page being recorded.
    page: RawPage,
    /// The whole page, the clip when no clip path is active.
    page_rect: kurbo::Rect,
    /// Stack of active clip rectangles (bounding boxes of the clip paths,
    /// each intersected with the one below).
    clips: Vec<kurbo::Rect>,
}

/// Position, size and direction of a glyph on the page, from its transform.
struct GlyphPlacement {
    /// Glyph origin on the page.
    origin: Point,
    /// Font size in points (vertical scale of the transform).
    size: f32,
    /// Horizontal scale of the transform, in points per em.
    x_scale: f64,
    /// Rotation of the baseline in degrees.
    angle: f32,
    /// Direction of the baseline.
    dir: Dir,
}

impl GlyphPlacement {
    /// Reads the placement of a glyph from its transform (glyph space to
    /// page). Returns `None` for glyphs whose origin lies outside `clip`
    /// (with a small margin) and for glyphs too small to be real text.
    fn new(transform: Affine, clip: kurbo::Rect) -> Option<Self> {
        let origin = transform * Point::ZERO;
        if origin.x < clip.x0 - GLYPH_CLIP_MARGIN_PT
            || origin.x > clip.x1 + GLYPH_CLIP_MARGIN_PT
            || origin.y < clip.y0 - GLYPH_CLIP_MARGIN_PT
            || origin.y > clip.y1 + GLYPH_CLIP_MARGIN_PT
        {
            return None;
        }
        let [xx, yx, xy, yy, _, _] = transform.as_coeffs();
        let size = (xy.hypot(yy) * GLYPH_UNITS_PER_EM) as f32;
        let x_scale = xx.hypot(yx) * GLYPH_UNITS_PER_EM;
        if size < MIN_GLYPH_SIZE_PT || !size.is_finite() {
            return None;
        }
        let angle = (yx.atan2(xx)).to_degrees() as f32;
        let dir = if angle.abs() < HORIZONTAL_MAX_ANGLE_DEG {
            Dir::Horizontal
        } else {
            Dir::Rotated
        };
        Some(Self {
            origin,
            size,
            x_scale,
            angle,
            dir,
        })
    }
}

impl Extractor<'_, '_> {
    /// The active clip rectangle.
    fn clip(&self) -> kurbo::Rect {
        self.clips.last().copied().unwrap_or(self.page_rect)
    }

    /// The part of `bbox` inside the active clip, or `None` when they do
    /// not touch. Boxes of zero area (straight lines) are kept when they
    /// touch the clip.
    fn visible(&self, bbox: kurbo::Rect) -> Option<Rect> {
        let clip = self.clip();
        let inside = bbox.intersect(clip);
        // `intersect` of disjoint rects yields a zero-area rect, lines also
        // have zero area, so test for overlap explicitly.
        if bbox.x0 > clip.x1 || bbox.x1 < clip.x0 || bbox.y0 > clip.y1 || bbox.y1 < clip.y0 {
            return None;
        }
        Some(Rect::new(
            inside.x0 as f32,
            inside.y0 as f32,
            inside.x1 as f32,
            inside.y1 as f32,
        ))
    }

    /// Records a one-pixel stencil mask as a filled path of the box it is
    /// stretched over, in the colour it is painted with. Masks that do not
    /// paint their pixel (or are not visible) leave nothing.
    fn record_stencil_rule(&mut self, stencil: &hayro_interpret::StencilImage<'_, '_>, bbox: kurbo::Rect) {
        let mut painted = false;
        let mut color = [0, 0, 0, 255];
        stencil.with_stencil(
            |luma, paint| {
                painted = luma.data.first().is_some_and(|&sample| sample != 0);
                color = paint_color(paint);
            },
            None,
        );
        if !painted || color[3] == 0 {
            return;
        }
        let Some(bbox) = self.visible(bbox) else {
            return;
        };
        self.page.paths.push(PathBox {
            bbox,
            stroked: false,
            filled: true,
            color,
            line_width: 0.0,
            elements: STENCIL_RULE_ELEMENTS,
        });
    }

    /// Text, advance (in glyph units), font index and outline box (in glyph
    /// space) of a glyph. Outline glyphs are identified through the font
    /// registry; Type 3 glyphs have no outline and an unknown font.
    fn describe_glyph(&mut self, glyph: &PdfGlyph<'_>) -> (Option<BfString>, Option<f32>, u16, Option<kurbo::Rect>) {
        match glyph {
            PdfGlyph::Outline(outline) => (
                outline.as_unicode(),
                outline.advance_width(),
                self.fonts.identify(outline),
                self.fonts.outline_bbox(outline),
            ),
            PdfGlyph::Type3(type3) => (type3.as_unicode(), type3.advance_width(), UNKNOWN_FONT, None),
        }
    }
}

impl<'a> Device<'a> for Extractor<'_, 'a> {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, draw_mode: &DrawMode) {
        let (stroked, filled, line_width) = match draw_mode {
            DrawMode::Fill(_) => (false, true, 0.0),
            DrawMode::Stroke(stroke) => (true, false, stroke.line_width),
            DrawMode::FillAndStroke(_, stroke) => (true, true, stroke.line_width),
            DrawMode::Invisible => return,
        };
        let color = paint_color(&props.paint);
        if color[3] == 0 {
            return;
        }
        let [xx, yx, xy, yy, _, _] = props.transform.as_coeffs();
        let scale = ((xx * yy - yx * xy).abs()).sqrt();
        let line_width_page = line_width as f64 * scale;
        let mut bbox = props.transform.transform_rect_bbox(path.bounding_box());
        if stroked {
            bbox = bbox.inflate(line_width_page * 0.5, line_width_page * 0.5);
        }
        let Some(bbox) = self.visible(bbox) else {
            return;
        };
        self.page.paths.push(PathBox {
            bbox,
            stroked,
            filled,
            color,
            line_width: line_width_page as f32,
            elements: path.elements().len().min(u16::MAX as usize) as u16,
        });
    }

    fn push_clip_path(&mut self, clip_path: &ClipPath) {
        let bbox = clip_path.path.bounding_box();
        let current = self.clip();
        let mut clip = bbox.intersect(current);
        if bbox.x0 > current.x1 || bbox.x1 < current.x0 || bbox.y0 > current.y1 || bbox.y1 < current.y0 {
            // Empty clip: nothing drawn inside is visible.
            clip = EMPTY_CLIP;
        }
        self.clips.push(clip);
    }

    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}

    fn draw_glyph_run(&mut self, glyph_run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, draw_mode: &DrawMode) {
        let invisible = matches!(draw_mode, DrawMode::Invisible);
        let color = paint_color(&props.paint);
        let clip = self.clip();
        for glyph in glyph_run.glyphs() {
            let transform: Affine = props.transform * glyph.transform();
            let Some(placement) = GlyphPlacement::new(transform, clip) else {
                continue;
            };
            let (unicode, advance, font, outline_box) = self.describe_glyph(glyph);
            let info = &self.fonts.infos[font as usize];
            let mut text = match info.mathabx {
                Some(abx) => abx.text(glyph.char_code()).unwrap_or("\u{FFFD}").to_string(),
                None => glyph_text(unicode),
            };
            if text.is_empty() {
                continue;
            }
            if self.fonts.infos[font as usize].math_pi {
                text = remap_math_pi(&text);
            }
            if self.fonts.infos[font as usize].math {
                text = remap_tex_math(&text);
            }
            if self.fonts.infos[font as usize].radical_pi {
                // The radical sign and the pieces of its bar, encoded on
                // letters: the bar is drawn, not read.
                text = match text.as_str() {
                    "p" => "√".into(),
                    "ffi" | "ff" | "fi" | "fl" => continue,
                    _ => text,
                };
            }
            if text == "=" && outline_box.is_some_and(|outline| outline.height() > SLASH_MIN_ASPECT * outline.width()) {
                text = "/".into();
            }
            // Use the horizontal extent of the transform so that horizontally
            // scaled text keeps correct advances.
            let adv = advance.unwrap_or(DEFAULT_ADVANCE_UNITS) as f64 / GLYPH_UNITS_PER_EM * placement.x_scale;
            let ink = ink_box(outline_box, transform, &placement, adv);
            self.page.glyphs.push(Glyph {
                text,
                x: placement.origin.x as f32,
                y: placement.origin.y as f32,
                adv: adv as f32,
                size: placement.size,
                font,
                dir: placement.dir,
                angle: placement.angle,
                invisible,
                color: [color[0], color[1], color[2]],
                ink,
            });
        }
    }

    fn draw_image(&mut self, image: Image<'a, '_>, props: ImageDrawProps<'a>) {
        let (px_width, px_height) = (image.width(), image.height());
        let rect = kurbo::Rect::new(0.0, 0.0, px_width as f64, px_height as f64);
        let bbox = props.transform.transform_rect_bbox(rect);
        // A tiny stencil mask stretched thin is a rule (table rules in some
        // typesetting): it takes part in float and table detection.
        let mut rule = false;
        if px_width * px_height <= STENCIL_RULE_MAX_PIXELS
            && let Image::Stencil(stencil) = &image
        {
            self.record_stencil_rule(stencil, bbox);
            rule = true;
        }
        // A 1x1 image stretched into a bar is drawn as a rectangle in the
        // hard-95 page picture and gives table rules (see `pixel_rules`).
        if (px_width, px_height) == (1, 1)
            && let Some(bar) = self.visible(bbox)
            && bar.width().min(bar.height()) <= PIXEL_RULE_MAX_THICKNESS_PT
            && bar.width().max(bar.height()) > MIN_IMAGE_SIDE_PT
        {
            self.page.pixel_rules.push(PathBox {
                bbox: bar,
                stroked: false,
                filled: true,
                color: PIXEL_RULE_COLOR,
                line_width: 0.0,
                elements: 5,
            });
            rule = true;
        }
        if rule {
            return;
        }
        if let Some(bbox) = self.visible(bbox)
            && bbox.width() > MIN_IMAGE_SIDE_PT
            && bbox.height() > MIN_IMAGE_SIDE_PT
        {
            self.page.images.push(ImageBox {
                bbox,
                px_width,
                px_height,
            });
        }
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn pop_transparency_group(&mut self) {}
}

/// Interprets one page and returns its raw content.
///
/// `index` is the zero-based page number recorded in the result. `fonts`
/// is switched to the fonts of this page (see
/// [`FontRegistry::begin_page`]) and learns any new fonts; `cache` and
/// `settings` are the interpreter's, reused across the pages of one thread.
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

/// RGBA colour of a paint; pattern paints get [`PATTERN_COLOR`].
fn paint_color(paint: &Paint<'_>) -> [u8; 4] {
    match paint {
        Paint::Color(color) => color.to_rgba().to_rgba8(),
        Paint::Pattern(_) => PATTERN_COLOR,
    }
}

/// The normalised text of a glyph's Unicode mapping: U+FFFD when there is
/// none, and empty when the mapping holds only characters that are dropped
/// (control characters, zero-width spaces).
fn glyph_text(unicode: Option<BfString>) -> String {
    let mut text = String::new();
    match unicode {
        Some(BfString::Char(ch)) => {
            let mut buf = [0u8; 4];
            normalize_text(ch.encode_utf8(&mut buf), &mut text);
        }
        Some(BfString::String(string)) => normalize_text(&string, &mut text),
        None => text.push('\u{FFFD}'),
    }
    text
}

/// Appends `text` to `out`, expanding Latin ligatures, turning special
/// spaces into plain spaces and soft or non-breaking hyphens into `-`, and
/// dropping control characters, zero-width characters and the byte-order
/// mark, so that text from different PDF producers compares equal.
/// Private-use characters (a ligature such as "ft" that the font maps to a
/// code of its own) become U+FFFD: no other font draws them, so the glyph
/// must stay in the page picture.
fn normalize_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
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
            '\u{E000}'..='\u{F8FF}' => out.push('\u{FFFD}'),
            other => out.push(other),
        }
    }
}

/// Repairs the Unicode values that common TeX math fonts are given in
/// PDFs: the lunate epsilon of cmmi comes out as "ǫ" (o with ogonek) and the
/// box of msam as "✷" (an eight-spoked asterisk).
fn remap_tex_math(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{01EB}' => '\u{03F5}',
            '\u{2737}' => '\u{25A1}',
            other => other,
        })
        .collect()
}

/// Undoes the encoding of "Mathematical Pi" fonts, which put math symbols
/// on Latin-1 letters: maps the letters back to the symbols they draw.
fn remap_math_pi(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '¼' => '=',
            'þ' => '+',
            'ð' => '(',
            'Þ' => ')',
            '½' => '[',
            'f' => '{',
            'g' => '}',
            other => other,
        })
        .collect()
}

/// Ink box of a glyph on the page: its outline box mapped through the
/// glyph's transform, or, for glyphs without an outline, a box of typical
/// proportions from the origin over the advance `adv`.
fn ink_box(outline_box: Option<kurbo::Rect>, transform: Affine, placement: &GlyphPlacement, adv: f64) -> Rect {
    let origin = placement.origin;
    let size = placement.size;
    match outline_box {
        Some(outline) => {
            let page_box = transform.transform_rect_bbox(outline);
            Rect::new(
                page_box.x0 as f32,
                page_box.y0 as f32,
                page_box.x1 as f32,
                page_box.y1 as f32,
            )
        }
        None => Rect::new(
            origin.x as f32,
            origin.y as f32 - FALLBACK_INK_ASCENT_EM * size,
            (origin.x + adv) as f32,
            origin.y as f32 + FALLBACK_INK_DESCENT_EM * size,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_text;

    #[test]
    fn private_use_ligature_is_unknown() {
        // Linux Libertine maps its "ft" ligature to U+E039.
        let mut text = String::new();
        normalize_text("le\u{E039}most", &mut text);
        assert_eq!(text, "le\u{FFFD}most");
    }
}
