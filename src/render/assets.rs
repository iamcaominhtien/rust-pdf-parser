//! Pictures used by the HTML (page backgrounds, figures, equations), drawn
//! after the HTML is written, on several threads.

use super::*;
use crate::backend::PageRenderer;
use crate::backend::PdfFile;
use crate::model::*;
use base64::Engine;
use std::collections::{HashMap, HashSet};

/// Marks a placeholder; a control character never written to the HTML.
pub(crate) const MARK: char = '\u{1}';

/// Grid steps per PDF point used to match glyph origins: glyphs drawn as
/// HTML are identified by their origin rounded to a quarter point.
const ORIGIN_STEPS_PER_PT: f32 = 4.0;

/// Highest pixels-per-point scale of a PNG page background: whole pages at
/// a higher scale make very large files for little visible gain.
const MAX_PAGE_PNG_SCALE: f32 = 2.5;

/// Prefix of an SVG line that draws a raster picture.
const SVG_IMAGE_PREFIX: &str = "<image ";
/// Start of the `src` of a PNG picture in an SVG, up to the payload.
const PNG_PIXEL_MARK: &str = "data:image/png;base64,";
/// The stretched-pixel pictures that [`pixels_to_rects`] replaces have a
/// payload shorter than this (in base64 characters): a 1 by 1 PNG is about
/// 100 characters long.
const PIXEL_PAYLOAD_MAX_LEN: usize = 400;

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

    /// The `width` and `height` attributes of the picture of a page region
    /// (its pixel size): the browser keeps its place, at the right height,
    /// before it loads.
    pub fn region_size_attrs(&self, spec: &ImageSpec) -> String {
        let (width, height) = self.region_size(spec);
        format!(" width=\"{width}\" height=\"{height}\"")
    }

    /// The pixel size of the picture of a page region.
    pub fn region_size(&self, spec: &ImageSpec) -> (u32, u32) {
        let (_, width, height) = crate::backend::render::region_bitmap(spec.rect, self.opts.scale);
        (width, height)
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
                let svg = pixels_to_rects(renderer.page_svg_without_text(*page, &remove)?);
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

/// Replaces the pictures of a single pixel stretched over an area, which
/// some typesetters use for table rules, by filled rectangles of the same
/// colour. A browser draws a pixelated picture thinner than a pixel as
/// nothing at all, so the rules of such tables would disappear from the page
/// picture; a rectangle that thin is drawn as a faint line, like the PDF.
///
/// Relies on hayro's SVG layout: one `<image transform="..."
/// xlink:href="data:image/png;base64,..." width="1" height="1" .../>` per
/// line. Other pictures and other lines are left alone.
fn pixels_to_rects(svg: String) -> String {
    if !svg.contains(PNG_PIXEL_MARK) {
        return svg;
    }
    let mut colors: HashMap<&str, Option<String>> = HashMap::new();
    let mut out = String::with_capacity(svg.len());
    for line in svg.lines() {
        let rect = line
            .strip_prefix(SVG_IMAGE_PREFIX)
            .filter(|rest| rest.contains("width=\"1\" height=\"1\""))
            .and_then(|rest| {
                let transform = attribute(rest, "transform")?;
                let payload = rest[rest.find(PNG_PIXEL_MARK)? + PNG_PIXEL_MARK.len()..]
                    .split('"')
                    .next()?;
                let fill = colors.entry(payload).or_insert_with(|| pixel_fill(payload)).as_ref()?;
                Some(format!(
                    "<rect transform=\"{transform}\" width=\"1\" height=\"1\" {fill}/>"
                ))
            });
        out.push_str(rect.as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

/// The value of the attribute `name` in the text of an SVG element.
fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let start = element.find(&format!("{name}=\""))? + name.len() + 2;
    element[start..].split('"').next()
}

/// The `fill` attributes that colour a rectangle like the 1 by 1 PNG
/// `payload` (base64), or `None` when it is no such picture.
fn pixel_fill(payload: &str) -> Option<String> {
    if payload.len() > PIXEL_PAYLOAD_MAX_LEN {
        return None;
    }
    let png = base64::engine::general_purpose::STANDARD.decode(payload).ok()?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok()?;
    if (img.width(), img.height()) != (1, 1) {
        return None;
    }
    let [red, green, blue, alpha] = img.to_rgba8().get_pixel(0, 0).0;
    let opacity = if alpha == u8::MAX {
        String::new()
    } else {
        format!(" fill-opacity=\"{:.3}\"", f32::from(alpha) / 255.0)
    };
    Some(format!("fill=\"rgb({red},{green},{blue})\"{opacity}"))
}

/// The key of a glyph origin (`x`, `y` in PDF points) in the sets of glyphs
/// drawn as HTML: the origin rounded to the nearest quarter point.
pub(crate) fn origin_key(x: f32, y: f32) -> (i32, i32) {
    (
        (x * ORIGIN_STEPS_PER_PT).round() as i32,
        (y * ORIGIN_STEPS_PER_PT).round() as i32,
    )
}
