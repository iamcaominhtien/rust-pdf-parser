//! Rasterisation of pages and page regions, and SVG export of whole pages.

use crate::common::geom::Rect;
use hayro::vello_cpu::color::palette::css::{TRANSPARENT, WHITE};
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings, render_into};
use hayro_interpret::InterpreterSettings;
use hayro_interpret::util::TransformExt;
use hayro_syntax::Pdf;
use kurbo::Affine;

pub struct PageRenderer<'a> {
    pdf: &'a Pdf,
    cache: RenderCache<'a>,
    svg_cache: hayro_svg::RenderCache<'a>,
    settings: InterpreterSettings,
}

impl<'a> PageRenderer<'a> {
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
    pub fn region_png(&self, page: usize, rect: Rect, scale: f32, transparent: bool) -> Option<(Vec<u8>, u32, u32)> {
        let page = self.pdf.pages().get(page)?;
        // Keep the bitmap within sane bounds.
        let max_dim = 6000.0;
        let scale = scale
            .min(max_dim / rect.width().max(1.0))
            .min(max_dim / rect.height().max(1.0));
        let w = (rect.width() * scale).ceil().max(1.0) as u16;
        let h = (rect.height() * scale).ceil().max(1.0) as u16;
        let mut ctx = RenderContext::new(w, h);
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
        let mut pixmap = Pixmap::new(w, h);
        ctx.render_with(
            &mut pixmap,
            &mut Resources::default(),
            RasterizerSettings {
                target_init: TargetInit::Clear(if transparent { TRANSPARENT } else { WHITE }),
                ..Default::default()
            },
        );
        let png = pixmap.into_png().ok()?;
        Some((png, w as u32, h as u32))
    }

    pub fn page_png(&self, page: usize, scale: f32) -> Option<(Vec<u8>, u32, u32)> {
        let p = self.pdf.pages().get(page)?;
        let (w, h) = p.render_dimensions();
        self.region_png(page, Rect::new(0.0, 0.0, w, h), scale, false)
    }

    /// Like [`Self::page_svg`], but without the glyphs whose origin
    /// `remove(x, y)` selects. Used to draw a page whose text is supplied
    /// separately as HTML. Glyph outlines that are no longer referenced are
    /// dropped as well.
    pub fn page_svg_without_text(&self, page: usize, remove: &dyn Fn(f32, f32) -> bool) -> Option<String> {
        let svg = self.page_svg(page)?;

        // Glyphs are written one per line as
        // `<use xlink:href="#g12" transform="matrix(a b c d e f)" .../>`
        // where (e, f) is the glyph origin on the page.
        fn glyph_use(line: &str) -> Option<(&str, f32, f32)> {
            let t = line.trim_start();
            let rest = t.strip_prefix("<use xlink:href=\"#")?;
            let id = &rest[..rest.find('"')?];
            let tr = &rest[rest.find("transform=\"")? + 11..];
            let tr = &tr[..tr.find('"')?];
            let inner = tr
                .strip_prefix("matrix(")
                .or_else(|| tr.strip_prefix("translate("))?
                .strip_suffix(')')?;
            let nums: Vec<f32> = inner
                .split([' ', ','])
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse().ok())
                .collect();
            let n = nums.len();
            (n >= 2).then(|| (id, nums[n - 2], nums[n - 1]))
        }
        fn glyph_def(line: &str) -> Option<&str> {
            let rest = line.trim_start().strip_prefix("<path id=\"")?;
            let id = &rest[..rest.find('"')?];
            (id.starts_with('g') && id[1..].chars().all(|c| c.is_ascii_digit()) && id.len() > 1).then_some(id)
        }

        // Blank glyphs (spaces) draw nothing and can always go.
        let blank: std::collections::HashSet<&str> = svg
            .lines()
            .filter(|l| l.trim_end().ends_with(" d=\"\"/>"))
            .filter_map(glyph_def)
            .collect();

        let mut drop = Vec::new();
        let mut referenced: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for line in svg.lines() {
            match glyph_use(line) {
                Some((id, x, y)) if blank.contains(id) || remove(x, y) => drop.push(true),
                Some((id, _, _)) => {
                    referenced.insert(id);
                    drop.push(false);
                }
                None => drop.push(false),
            }
        }
        let mut out = String::with_capacity(svg.len() / 2);
        for (line, dropped) in svg.lines().zip(drop) {
            if dropped || glyph_def(line).is_some_and(|id| !referenced.contains(id)) {
                continue;
            }
            out.push_str(line.trim_start());
            out.push('\n');
        }
        Some(out)
    }

    pub fn page_svg(&self, page: usize) -> Option<String> {
        let p = self.pdf.pages().get(page)?;
        let svg = hayro_svg::convert(
            p,
            &self.svg_cache,
            &self.settings,
            &hayro_svg::SvgRenderSettings {
                bg_color: [255, 255, 255, 255],
            },
        );
        Some(compact_images(svg))
    }
}

/// Photographs are embedded in the SVG as lossless PNG, which can be
/// enormous. Opaque images above a size threshold are re-encoded as JPEG
/// (and scaled down if they are far larger than any screen needs).
fn compact_images(svg: String) -> String {
    use base64::Engine;
    const MARKER: &str = "data:image/png;base64,";
    const MIN_LEN: usize = 200_000;
    const MAX_SIDE: u32 = 2600;
    if !svg.contains(MARKER) {
        return svg;
    }
    let engine = base64::engine::general_purpose::STANDARD;
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg.as_str();
    while let Some(pos) = rest.find(MARKER) {
        let start = pos + MARKER.len();
        let len = rest[start..].find('"').unwrap_or(rest.len() - start);
        let payload = &rest[start..start + len];
        let replacement = (len >= MIN_LEN)
            .then(|| engine.decode(payload).ok())
            .flatten()
            .and_then(|png| image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok())
            .and_then(|img| {
                // Transparency cannot be kept in a JPEG.
                if img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] != 255) {
                    return None;
                }
                let img = if img.width().max(img.height()) > MAX_SIDE {
                    img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
                } else {
                    img
                };
                let mut jpeg = Vec::new();
                let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 88);
                img.to_rgb8().write_with_encoder(encoder).ok()?;
                Some(jpeg)
            })
            .filter(|jpeg| jpeg.len() * 4 / 3 < len);
        out.push_str(&rest[..pos]);
        match replacement {
            Some(jpeg) => {
                out.push_str("data:image/jpeg;base64,");
                out.push_str(&engine.encode(jpeg));
            }
            None => {
                out.push_str(MARKER);
                out.push_str(payload);
            }
        }
        rest = &rest[start + len..];
    }
    out.push_str(rest);
    out
}
