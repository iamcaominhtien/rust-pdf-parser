//! Pictures used by the HTML (page backgrounds, figures, equations), drawn
//! after the HTML is written, on several threads.

use super::*;
use crate::backend::PageRenderer;
use crate::model::*;
use base64::Engine;
use hayro_syntax::Pdf;
use std::collections::HashSet;

/// A picture the HTML needs: a page region, or a whole page.
pub(crate) enum ImageJob {
    Region(ImageSpec, String),
    Page(usize),
    /// A page without the glyphs whose origins (in quarter points) are in
    /// the set: their text is drawn as HTML.
    PageWithoutText(usize, HashSet<(i32, i32)>),
}

/// Collects the pictures the HTML needs. While the HTML is written, each
/// picture is only recorded and a placeholder takes the place of its
/// `src`; [`ImageStore::finish`] then draws all pictures on several threads
/// and puts their data (or file names) in place.
pub(crate) struct ImageStore<'r> {
    pub(crate) opts: &'r HtmlOptions,
    pub(crate) jobs: Vec<ImageJob>,
}

/// Marks a placeholder; a control character never written to the HTML.
pub(crate) const MARK: char = '\u{1}';

impl ImageStore<'_> {
    pub(crate) fn add(&mut self, job: ImageJob) -> String {
        self.jobs.push(job);
        format!("{MARK}{}{MARK}", self.jobs.len() - 1)
    }

    /// Renders a page region and returns the `src` of the image.
    pub fn region(&mut self, spec: &ImageSpec, hint: &str) -> String {
        self.add(ImageJob::Region(*spec, hint.to_string()))
    }

    /// The `src` of a picture of a page without the glyphs whose origins,
    /// rounded to a quarter point, are in `drawn` (or next to one of them).
    pub fn page_without_text(&mut self, page: usize, drawn: HashSet<(i32, i32)>) -> String {
        self.add(ImageJob::PageWithoutText(page, drawn))
    }

    /// The `src` of a picture of a whole page (hard mode background).
    pub fn page(&mut self, page: usize) -> String {
        self.add(ImageJob::Page(page))
    }

    /// Draws one picture: (data, mime type, file extension, name hint).
    pub(crate) fn draw(
        &self,
        r: &PageRenderer<'_>,
        job: &ImageJob,
    ) -> Option<(Vec<u8>, &'static str, &'static str, String)> {
        let page_hint = |p: usize| format!("page-{}", p + 1);
        match job {
            ImageJob::Region(spec, hint) => {
                let (png, _, _) = r.region_png(spec.page, spec.rect, self.opts.scale, spec.transparent)?;
                Some((png, "image/png", "png", hint.clone()))
            }
            ImageJob::Page(p) => match self.opts.page_image {
                PageImage::Svg => Some((r.page_svg(*p)?.into_bytes(), "image/svg+xml", "svg", page_hint(*p))),
                PageImage::Png => {
                    let (png, _, _) = r.page_png(*p, self.opts.scale.min(2.5))?;
                    Some((png, "image/png", "png", page_hint(*p)))
                }
            },
            ImageJob::PageWithoutText(p, drawn) => {
                let remove = |x: f32, y: f32| {
                    let (qx, qy) = ((x * 4.0).round() as i32, (y * 4.0).round() as i32);
                    (-1..=1).any(|dx| (-1..=1).any(|dy| drawn.contains(&(qx + dx, qy + dy))))
                };
                let svg = r.page_svg_without_text(*p, &remove)?;
                Some((svg.into_bytes(), "image/svg+xml", "svg", page_hint(*p)))
            }
        }
    }

    /// Draws all recorded pictures and replaces the placeholders in `html`.
    pub(crate) fn finish(self, pdf: &Pdf, html: String) -> HtmlOutput {
        let drawn = crate::common::parallel::map(
            self.jobs.len(),
            self.opts.jobs,
            || PageRenderer::new(pdf),
            |r, i| self.draw(r, &self.jobs[i]),
        );
        let mut drawn: Vec<_> = drawn.into_iter().map(Some).collect();
        let mut files = Vec::new();
        let mut out = String::with_capacity(html.len());
        let mut rest = html.as_str();
        while let Some(start) = rest.find(MARK) {
            out.push_str(&rest[..start]);
            let after = &rest[start + MARK.len_utf8()..];
            let end = after.find(MARK).expect("placeholders are closed");
            let index: usize = after[..end].parse().expect("placeholder index");
            // A picture that cannot be drawn leaves an empty `src`.
            if let Some((data, mime, ext, hint)) = drawn[index].take().flatten() {
                if self.opts.inline_images {
                    out.push_str("data:");
                    out.push_str(mime);
                    out.push_str(";base64,");
                    base64::engine::general_purpose::STANDARD.encode_string(&data, &mut out);
                } else {
                    let name = format!("{}/{:03}-{}.{}", self.opts.asset_dir, files.len() + 1, hint, ext);
                    out.push_str(&name);
                    files.push((name, data));
                }
            }
            rest = &after[end + MARK.len_utf8()..];
        }
        out.push_str(rest);
        HtmlOutput { html: out, files }
    }
}
