//! Rasterisation of pages and page regions, and SVG export of whole pages.

use crate::common::geom::Rect;
use base64::Engine;
use hayro::vello_cpu::color::palette::css::{TRANSPARENT, WHITE};
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings, render_into};
use hayro_interpret::InterpreterSettings;
use hayro_interpret::util::TransformExt;
use hayro_syntax::Pdf;
use kurbo::Affine;
use std::collections::HashSet;

/// Largest width or height of a rendered bitmap, in pixels: the scale is
/// reduced for regions that would exceed it, to keep memory use sane.
const MAX_BITMAP_SIDE_PX: f32 = 6000.0;
/// Background of exported SVG pages (opaque white, as on paper).
const SVG_BACKGROUND: [u8; 4] = [255, 255, 255, 255];
/// Start of an embedded PNG image in the SVG output.
const PNG_DATA_URI: &str = "data:image/png;base64,";
/// Start of an embedded JPEG image, the replacement for [`PNG_DATA_URI`].
const JPEG_DATA_URI: &str = "data:image/jpeg;base64,";
/// Embedded PNGs shorter than this (in base64 characters, about 150 kB of
/// data) are left alone: recompressing them saves little.
const MIN_RECOMPRESS_LEN: usize = 200_000;
/// Recompressed images are scaled down to fit in a square of this many
/// pixels, which is more than any screen needs for one image.
const MAX_IMAGE_SIDE_PX: u32 = 2600;
/// JPEG quality of recompressed images: visually lossless for photographs
/// and plots at a fraction of the PNG size.
const JPEG_QUALITY: u8 = 88;
/// Start of the line that draws a glyph in hayro's SVG output.
const GLYPH_USE_PREFIX: &str = "<use xlink:href=\"#";
/// Attribute holding the placement of a glyph.
const TRANSFORM_ATTR: &str = "transform=\"";
/// Start of the line that defines a glyph outline in hayro's SVG output.
const GLYPH_DEF_PREFIX: &str = "<path id=\"";
/// End of the definition of a glyph that draws nothing (a space).
const BLANK_GLYPH_SUFFIX: &str = " d=\"\"/>";

/// Draws pages and page regions of one PDF. Keeps the render caches between
/// calls, so it should be reused for all the pictures a thread draws.
pub struct PageRenderer<'a> {
    /// The PDF to draw from.
    pdf: &'a Pdf,
    /// Cache of the bitmap renderer (decoded images, fonts, ...).
    cache: RenderCache<'a>,
    /// Cache of the SVG renderer.
    svg_cache: hayro_svg::RenderCache<'a>,
    /// Interpreter settings shared by both renderers.
    settings: InterpreterSettings,
}

impl<'a> PageRenderer<'a> {
    /// A renderer with empty caches for `pdf`.
    pub fn new(pdf: &'a Pdf) -> Self {
        Self {
            pdf,
            cache: RenderCache::new(),
            svg_cache: hayro_svg::RenderCache::new(),
            settings: InterpreterSettings::default(),
        }
    }

    /// Renders `rect` (page points, top-left origin) of a page at `scale`
    /// pixels per point and returns PNG data together with the pixel size.
    ///
    /// The scale is lowered when the bitmap would be larger than
    /// [`MAX_BITMAP_SIDE_PX`] on a side. With `transparent` the background is
    /// transparent instead of white. Returns `None` when the page does not
    /// exist or the PNG cannot be encoded.
    pub fn region_png(&self, page: usize, rect: Rect, scale: f32, transparent: bool) -> Option<(Vec<u8>, u32, u32)> {
        let page = self.pdf.pages().get(page)?;
        let (scale, width, height) = region_bitmap(rect, scale);
        let (width_px, height_px) = (width as u16, height as u16);
        let mut ctx = RenderContext::new(width_px, height_px);
        let transform = Affine::scale(scale as f64)
            * Affine::translate((-rect.x0 as f64, -rect.y0 as f64))
            * page.initial_transform(true).to_kurbo();
        render_into(
            page,
            &self.cache,
            &self.settings,
            &RenderSettings::default(),
            &mut ctx,
            transform,
        );
        ctx.flush();
        let mut pixmap = Pixmap::new(width_px, height_px);
        ctx.render_with(
            &mut pixmap,
            &mut Resources::default(),
            RasterizerSettings {
                target_init: TargetInit::Clear(if transparent { TRANSPARENT } else { WHITE }),
                ..Default::default()
            },
        );
        let png = pixmap.into_png().ok()?;
        Some((png, width_px as u32, height_px as u32))
    }

    /// Renders a whole page on white at `scale` pixels per point (see
    /// [`Self::region_png`]). Returns `None` when the page does not exist.
    pub fn page_png(&self, page: usize, scale: f32) -> Option<(Vec<u8>, u32, u32)> {
        let pdf_page = self.pdf.pages().get(page)?;
        let (width, height) = pdf_page.render_dimensions();
        self.region_png(page, Rect::new(0.0, 0.0, width, height), scale, false)
    }

    /// Like [`Self::page_svg`], but without the glyphs whose origin
    /// `remove(x, y)` selects. Used to draw a page whose text is supplied
    /// separately as HTML. Glyph outlines that are no longer referenced are
    /// dropped as well.
    ///
    /// Relies on hayro's SVG layout: one element per line, glyphs drawn by
    /// `<use>` elements referring to `<path id="gN">` definitions. Lines are
    /// written without their leading indentation.
    pub fn page_svg_without_text(&self, page: usize, remove: &dyn Fn(f32, f32) -> bool) -> Option<String> {
        let svg = self.page_svg(page)?;

        // Blank glyphs (spaces) draw nothing and can always go.
        let blank: HashSet<&str> = svg
            .lines()
            .filter(|line| line.trim_end().ends_with(BLANK_GLYPH_SUFFIX))
            .filter_map(glyph_def)
            .collect();

        let mut dropped_lines = Vec::new();
        let mut referenced: HashSet<&str> = HashSet::new();
        for line in svg.lines() {
            match glyph_use(line) {
                Some((id, x, y)) if blank.contains(id) || remove(x, y) => dropped_lines.push(true),
                Some((id, _, _)) => {
                    referenced.insert(id);
                    dropped_lines.push(false);
                }
                None => dropped_lines.push(false),
            }
        }
        let mut out = String::with_capacity(svg.len() / 2);
        for (line, dropped) in svg.lines().zip(dropped_lines) {
            if dropped || glyph_def(line).is_some_and(|id| !referenced.contains(id)) {
                continue;
            }
            out.push_str(line.trim_start());
            out.push('\n');
        }
        Some(out)
    }

    /// The page as an SVG document on a white background, with large opaque
    /// embedded images recompressed (see [`compact_images`]). Returns `None`
    /// when the page does not exist.
    pub fn page_svg(&self, page: usize) -> Option<String> {
        let pdf_page = self.pdf.pages().get(page)?;
        let svg = hayro_svg::convert(
            pdf_page,
            &self.svg_cache,
            &self.settings,
            &hayro_svg::SvgRenderSettings {
                bg_color: SVG_BACKGROUND,
            },
        );
        Some(compact_images(svg))
    }
}

/// Parses a line that draws a glyph: returns the referenced glyph id and
/// the glyph origin on the page, or `None` for any other line.
///
/// Glyphs are written one per line as
/// `<use xlink:href="#g12" transform="matrix(a b c d e f)" .../>`
/// (or `translate(e f)`), where (e, f) is the glyph origin on the page.
fn glyph_use(line: &str) -> Option<(&str, f32, f32)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix(GLYPH_USE_PREFIX)?;
    let id = &rest[..rest.find('"')?];
    let transform = &rest[rest.find(TRANSFORM_ATTR)? + TRANSFORM_ATTR.len()..];
    let transform = &transform[..transform.find('"')?];
    let inner = transform
        .strip_prefix("matrix(")
        .or_else(|| transform.strip_prefix("translate("))?
        .strip_suffix(')')?;
    let nums: Vec<f32> = inner
        .split([' ', ','])
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect();
    let n = nums.len();
    (n >= 2).then(|| (id, nums[n - 2], nums[n - 1]))
}

/// Parses a line that defines a glyph outline (`<path id="g12" .../>`):
/// returns the glyph id, or `None` for any other line (including paths
/// whose id is not `g` followed by digits).
fn glyph_def(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix(GLYPH_DEF_PREFIX)?;
    let id = &rest[..rest.find('"')?];
    (id.starts_with('g') && id[1..].chars().all(|ch| ch.is_ascii_digit()) && id.len() > 1).then_some(id)
}

/// Photographs are embedded in the SVG as lossless PNG, which can be
/// enormous. Opaque images above a size threshold are re-encoded as JPEG
/// (and scaled down if they are far larger than any screen needs).
///
/// An image is replaced only when its JPEG is smaller than the PNG; images
/// with any transparent pixel and images that cannot be decoded are kept.
fn compact_images(svg: String) -> String {
    if !svg.contains(PNG_DATA_URI) {
        return svg;
    }
    let engine = base64::engine::general_purpose::STANDARD;
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg.as_str();
    while let Some(pos) = rest.find(PNG_DATA_URI) {
        let start = pos + PNG_DATA_URI.len();
        let len = rest[start..].find('"').unwrap_or(rest.len() - start);
        let payload = &rest[start..start + len];
        let replacement = recompress_png(&engine, payload);
        out.push_str(&rest[..pos]);
        match replacement {
            Some(jpeg) => {
                out.push_str(JPEG_DATA_URI);
                out.push_str(&engine.encode(jpeg));
            }
            None => {
                out.push_str(PNG_DATA_URI);
                out.push_str(payload);
            }
        }
        rest = &rest[start + len..];
    }
    out.push_str(rest);
    out
}

/// The JPEG replacement for one base64 PNG `payload`, or `None` when the
/// image should stay as it is: too short to be worth it, not decodable,
/// not fully opaque, or not smaller as JPEG (compared in base64 length).
fn recompress_png(engine: &base64::engine::GeneralPurpose, payload: &str) -> Option<Vec<u8>> {
    let len = payload.len();
    (len >= MIN_RECOMPRESS_LEN)
        .then(|| engine.decode(payload).ok())
        .flatten()
        .and_then(|png| image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok())
        .and_then(|img| {
            // Transparency cannot be kept in a JPEG.
            if img.color().has_alpha() && img.to_rgba8().pixels().any(|pixel| pixel.0[3] != 255) {
                return None;
            }
            let img = if img.width().max(img.height()) > MAX_IMAGE_SIDE_PX {
                img.resize(
                    MAX_IMAGE_SIDE_PX,
                    MAX_IMAGE_SIDE_PX,
                    image::imageops::FilterType::Triangle,
                )
            } else {
                img
            };
            let mut jpeg = Vec::new();
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY);
            img.to_rgb8().write_with_encoder(encoder).ok()?;
            Some(jpeg)
        })
        // Base64 turns 3 bytes into 4 characters.
        .filter(|jpeg| jpeg.len() * 4 / 3 < len)
}

/// The scale and pixel size of the bitmap of `rect` (page points) drawn at
/// `scale` pixels per point by [`PageRenderer::region_png`]: the scale is
/// lowered so that no side exceeds [`MAX_BITMAP_SIDE_PX`]. The HTML gives
/// pictures this size before they are drawn, so that the page does not move
/// when they load.
pub fn region_bitmap(rect: Rect, scale: f32) -> (f32, u32, u32) {
    let scale = scale
        .min(MAX_BITMAP_SIDE_PX / rect.width().max(1.0))
        .min(MAX_BITMAP_SIDE_PX / rect.height().max(1.0));
    let width = (rect.width() * scale).ceil().max(1.0) as u32;
    let height = (rect.height() * scale).ceil().max(1.0) as u32;
    (scale, width, height)
}
