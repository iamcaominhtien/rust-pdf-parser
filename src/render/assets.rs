//! Pictures used by the HTML (page backgrounds, figures, equations), drawn
//! after the HTML is written, on several threads.

use super::*;
use crate::backend::PageRenderer;
use crate::backend::PdfFile;
use crate::model::*;
use base64::Engine;
use std::collections::HashSet;

/// Marks a placeholder; a control character never written to the HTML.
pub(crate) const MARK: char = '\u{1}';

/// Grid steps per PDF point used to match glyph origins: glyphs drawn as
/// HTML are identified by their origin rounded to a quarter point.
const ORIGIN_STEPS_PER_PT: f32 = 4.0;

/// Highest pixels-per-point scale of a PNG page background: whole pages at
/// a higher scale make very large files for little visible gain.
const MAX_PAGE_PNG_SCALE: f32 = 2.5;

/// A picture the HTML needs: a page region, or a whole page.
pub(crate) enum ImageJob {
    /// A region of a page (a figure or an equation), with a hint for the
    /// file name.
    Region(ImageSpec, String),
    /// A whole page with everything on it.
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
    /// The output options (scale, inline or external images, threads).
    pub(crate) opts: &'r HtmlOptions,
    /// The pictures recorded so far; a placeholder holds an index into it.
    pub(crate) jobs: Vec<ImageJob>,
}

/// A drawn picture: (data, mime type, file extension, name hint).
type DrawnImage = (Vec<u8>, &'static str, &'static str, String);

impl ImageStore<'_> {
    /// Records a picture and returns the placeholder that stands for its
    /// `src` until [`ImageStore::finish`].
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
    /// Returns `None` when the backend cannot draw it.
    pub(crate) fn draw(&self, renderer: &PageRenderer<'_>, job: &ImageJob) -> Option<DrawnImage> {
        let page_hint = |page: usize| format!("page-{}", page + 1);
        match job {
            ImageJob::Region(spec, hint) => {
                let (png, _, _) = renderer.region_png(spec.page, spec.rect, self.opts.scale, spec.transparent)?;
                Some((png, "image/png", "png", hint.clone()))
            }
            ImageJob::Page(page) => match self.opts.page_image {
                PageImage::Svg => Some((
                    renderer.page_svg(*page)?.into_bytes(),
                    "image/svg+xml",
                    "svg",
                    page_hint(*page),
                )),
                PageImage::Png => {
                    let (png, _, _) = renderer.page_png(*page, self.opts.scale.min(MAX_PAGE_PNG_SCALE))?;
                    Some((png, "image/png", "png", page_hint(*page)))
                }
            },
            ImageJob::PageWithoutText(page, drawn) => {
                // A glyph is removed when its origin, or a grid position next
                // to it, was drawn as HTML (rounding may differ by a step).
                let remove = |x: f32, y: f32| {
                    let (qx, qy) = origin_key(x, y);
                    (-1..=1).any(|dx| (-1..=1).any(|dy| drawn.contains(&(qx + dx, qy + dy))))
                };
                let svg = renderer.page_svg_without_text(*page, &remove)?;
                Some((svg.into_bytes(), "image/svg+xml", "svg", page_hint(*page)))
            }
        }
    }

    /// Draws all recorded pictures and replaces the placeholders in `html`.
    ///
    /// # Panics
    ///
    /// When `html` contains a placeholder that is not closed or whose index
    /// is not a number (it was not written by [`ImageStore::add`]).
    pub(crate) fn finish(self, pdf: &PdfFile, html: String) -> HtmlOutput {
        let drawn = crate::common::parallel::map(
            self.jobs.len(),
            self.opts.jobs,
            || pdf.renderer(),
            |renderer, i| self.draw(renderer, &self.jobs[i]),
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
            if let Some(image) = drawn[index].take().flatten() {
                self.put_image(image, &mut out, &mut files);
            }
            rest = &after[end + MARK.len_utf8()..];
        }
        out.push_str(rest);
        HtmlOutput { html: out, files }
    }

    /// Writes the `src` of a drawn picture to `out`: a data URI, or the name
    /// of a new file in the asset directory (which is added to `files`).
    fn put_image(&self, image: DrawnImage, out: &mut String, files: &mut Vec<(String, Vec<u8>)>) {
        let (data, mime, ext, hint) = image;
        if self.opts.inline_images {
            out.push_str("data:");
            out.push_str(mime);
            out.push_str(";base64,");
            base64::engine::general_purpose::STANDARD.encode_string(&data, out);
        } else {
            let name = format!("{}/{:03}-{}.{}", self.opts.asset_dir, files.len() + 1, hint, ext);
            out.push_str(&name);
            files.push((name, data));
        }
    }
}

/// The key of a glyph origin (`x`, `y` in PDF points) in the sets of glyphs
/// drawn as HTML: the origin rounded to the nearest quarter point.
pub(crate) fn origin_key(x: f32, y: f32) -> (i32, i32) {
    (
        (x * ORIGIN_STEPS_PER_PT).round() as i32,
        (y * ORIGIN_STEPS_PER_PT).round() as i32,
    )
}
