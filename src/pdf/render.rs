//! Rasterisation of pages and page regions, and SVG export of whole pages.

use crate::geom::Rect;
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
        render_into(page, &self.cache, &self.settings, &RenderSettings::default(), &mut ctx, transform);
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

    pub fn page_svg(&self, page: usize) -> Option<String> {
        let p = self.pdf.pages().get(page)?;
        Some(hayro_svg::convert(
            p,
            &self.svg_cache,
            &self.settings,
            &hayro_svg::SvgRenderSettings { bg_color: [255, 255, 255, 255] },
        ))
    }
}
