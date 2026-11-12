//! HTML output in three fidelity modes.

mod friendly;
mod hard;
mod medium;

use crate::document::Analysis;
use crate::model::*;
use crate::pdf::PageRenderer;
use base64::Engine;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Facsimile: every page looks exactly like the PDF, with a selectable
    /// text layer organised by paragraph.
    Hard,
    /// Page and column structure preserved, text reflowed as real HTML.
    Medium,
    /// Single-column, responsive article optimised for reading.
    Friendly,
}

impl Mode {
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Hard => "hard",
            Mode::Medium => "medium",
            Mode::Friendly => "friendly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageImage {
    Svg,
    Png,
}

#[derive(Debug, Clone)]
pub struct HtmlOptions {
    pub mode: Mode,
    /// Embed images as data URIs (single self-contained file) instead of
    /// writing them next to the HTML file.
    pub inline_images: bool,
    /// Directory name (relative to the HTML file) for external images.
    pub asset_dir: String,
    /// Pixels per PDF point for figure and equation images.
    pub scale: f32,
    /// Background of the pages in hard mode.
    pub page_image: PageImage,
}

impl Default for HtmlOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Friendly,
            inline_images: true,
            asset_dir: "assets".into(),
            scale: 3.0,
            page_image: PageImage::Svg,
        }
    }
}

pub struct HtmlOutput {
    pub html: String,
    /// Files to write next to the HTML: (relative path, contents).
    pub files: Vec<(String, Vec<u8>)>,
}

pub fn render(analysis: &Analysis, renderer: &PageRenderer<'_>, opts: &HtmlOptions) -> HtmlOutput {
    let mut images = ImageStore {
        renderer,
        opts,
        files: Vec::new(),
        counter: 0,
    };
    let html = match opts.mode {
        Mode::Friendly => friendly::render(&analysis.doc, &mut images),
        Mode::Medium => medium::render(&analysis.doc, &mut images),
        Mode::Hard => hard::render(analysis, &mut images),
    };
    HtmlOutput { html, files: images.files }
}

pub(crate) struct ImageStore<'r, 'a> {
    renderer: &'r PageRenderer<'a>,
    opts: &'r HtmlOptions,
    files: Vec<(String, Vec<u8>)>,
    counter: usize,
}

impl ImageStore<'_, '_> {
    fn emit(&mut self, data: Vec<u8>, mime: &str, ext: &str, hint: &str) -> String {
        if self.opts.inline_images {
            format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&data))
        } else {
            self.counter += 1;
            let name = format!("{}/{:03}-{}.{}", self.opts.asset_dir, self.counter, hint, ext);
            self.files.push((name.clone(), data));
            name
        }
    }

    /// Renders a page region and returns the `src` of the image.
    pub fn region(&mut self, spec: &ImageSpec, hint: &str) -> Option<String> {
        let (png, _, _) = self
            .renderer
            .region_png(spec.page, spec.rect, self.opts.scale, spec.transparent)?;
        Some(self.emit(png, "image/png", "png", hint))
    }

    /// Renders a whole page (hard mode background).
    pub fn page(&mut self, page: usize) -> Option<String> {
        match self.opts.page_image {
            PageImage::Svg => {
                let svg = self.renderer.page_svg(page)?;
                Some(self.emit(svg.into_bytes(), "image/svg+xml", "svg", &format!("page-{}", page + 1)))
            }
            PageImage::Png => {
                let (png, _, _) = self.renderer.page_png(page, self.opts.scale.min(2.5))?;
                Some(self.emit(png, "image/png", "png", &format!("page-{}", page + 1)))
            }
        }
    }
}

pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn spans_html(spans: &[Span]) -> String {
    let mut out = String::new();
    for s in spans {
        let mut open = String::new();
        let mut close = String::new();
        let mut tag = |name: &str| {
            open.push_str(&format!("<{name}>"));
            close.insert_str(0, &format!("</{name}>"));
        };
        if s.style & MONO != 0 {
            tag("code");
        }
        if s.style & BOLD != 0 {
            tag("strong");
        }
        if s.style & ITALIC != 0 {
            tag("em");
        }
        if s.style & SUP != 0 {
            tag("sup");
        }
        if s.style & SUB != 0 {
            tag("sub");
        }
        let body = format!("{open}{}{close}", esc(&s.text));
        match &s.href {
            Some(h) if h.starts_with('#') => out.push_str(&format!("<a class=\"xref\" href=\"{}\">{body}</a>", esc(h))),
            Some(h) => out.push_str(&format!(
                "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{body}</a>",
                esc(h)
            )),
            None => out.push_str(&body),
        }
    }
    out
}

pub(crate) fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect::<String>().trim().to_string()
}

static CAP_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:Supplementary\s+|Extended\s+Data\s+)?(?:Figure|Fig\.?|FIGURE|FIG\.?|Table|TABLE|Tab\.|Algorithm|ALGORITHM|Listing|Scheme|Chart|Plate)\s*\S+?\s*[:.|—–-]").unwrap()
});

/// Caption HTML with the label ("Figure 3:") emphasised.
pub(crate) fn caption_html(spans: &[Span]) -> String {
    if let Some(first) = spans.first() {
        if let Some(m) = CAP_LABEL.find(&first.text) {
            let mut rest = spans.to_vec();
            let label = first.text[..m.end()].to_string();
            rest[0].text = first.text[m.end()..].to_string();
            return format!("<span class=\"cap-label\">{}</span>{}", esc(&label), spans_html(&rest));
        }
    }
    spans_html(spans)
}

pub(crate) fn float_class(kind: crate::layout::floats::FloatKind) -> &'static str {
    use crate::layout::floats::FloatKind::*;
    match kind {
        Figure => "fig",
        Table => "tab",
        Algorithm => "alg",
        Graphic => "gfx",
    }
}

pub(crate) fn id_attr(b: &Block) -> String {
    match &b.anchor {
        Some(a) => format!(" id=\"{}\"", esc(a)),
        None => String::new(),
    }
}

pub(crate) fn doc_title(doc: &Document) -> String {
    doc.meta.title.clone().unwrap_or_else(|| "Untitled paper".to_string())
}

/// Alt text for a figure: its label plus any text found inside it.
pub(crate) fn figure_alt(b: &Block, label: &Option<String>) -> String {
    let mut alt = label.clone().unwrap_or_else(|| "Figure".into());
    if let Some(inner) = b.rows.first() {
        let t = plain(inner);
        if !t.is_empty() {
            alt.push_str(": ");
            alt.extend(t.chars().take(240));
        }
    }
    alt
}

/// How image widths are expressed in a given mode.
pub(crate) enum WidthUnit {
    /// Relative to the body font size (`pt_per_em` points per em).
    Em { pt_per_em: f32 },
    /// Relative to the page container width (`page_width` points wide).
    Cqw { page_width: f32 },
}

impl WidthUnit {
    fn css(&self, width_pt: f32) -> String {
        match self {
            WidthUnit::Em { pt_per_em } => format!("width:min(100%,{:.2}em)", width_pt / pt_per_em),
            WidthUnit::Cqw { page_width } => format!("width:min(100%,{:.2}cqw)", width_pt / page_width * 100.0),
        }
    }
}

pub(crate) struct FlowCtx {
    pub unit: WidthUnit,
}

fn heading_tag(level: u8) -> &'static str {
    match level {
        0 | 1 => "h2",
        2 => "h3",
        3 => "h4",
        _ => "h5",
    }
}

/// Renders a sequence of blocks as flowing HTML, grouping list items,
/// references and abstract paragraphs.
pub(crate) fn render_flow(blocks: &[&Block], images: &mut ImageStore<'_, '_>, ctx: &FlowCtx) -> String {
    #[derive(PartialEq, Clone, Copy)]
    enum Group {
        None,
        List,
        Refs,
        Abstract,
    }
    let mut out = String::new();
    let mut group = Group::None;
    let close = |g: Group, out: &mut String| match g {
        Group::List => out.push_str("</ul>\n"),
        Group::Refs => out.push_str("</ol>\n"),
        Group::Abstract => out.push_str("</section>\n"),
        Group::None => {}
    };
    for (bi, b) in blocks.iter().enumerate() {
        let want = match &b.kind {
            BlockKind::ListItem { .. } => Group::List,
            BlockKind::Reference { .. } => Group::Refs,
            BlockKind::Paragraph { in_abstract: true } => Group::Abstract,
            // Floats, equations and footnotes do not interrupt a group.
            BlockKind::Equation { .. } if group == Group::Abstract => group,
            _ => Group::None,
        };
        if want != group {
            close(group, &mut out);
            match want {
                Group::List => out.push_str("<ul class=\"list\">\n"),
                Group::Refs => out.push_str("<ol class=\"refs\">\n"),
                Group::Abstract => {
                    // The "Abstract" heading (if any) was emitted just before.
                    let had_heading = bi > 0
                        && matches!(blocks[bi - 1].kind, BlockKind::Heading { .. })
                        && plain(&blocks[bi - 1].spans).trim_end_matches(['.', ':']).eq_ignore_ascii_case("abstract");
                    out.push_str("<section class=\"abstract\">\n");
                    if !had_heading {
                        out.push_str("<div class=\"abstract-label\">Abstract</div>\n");
                    }
                }
                Group::None => {}
            }
            group = want;
        }
        let id = id_attr(b);
        match &b.kind {
            BlockKind::Title | BlockKind::Authors => {}
            BlockKind::Note => out.push_str(&format!("<p class=\"note\">{}</p>\n", spans_html(&b.spans))),
            BlockKind::Heading { level } => {
                let tag = heading_tag(*level);
                let is_abstract = plain(&b.spans).trim_end_matches(['.', ':']).eq_ignore_ascii_case("abstract");
                let cls = if is_abstract { " class=\"abstract-heading\"" } else { "" };
                out.push_str(&format!("<{tag}{id}{cls}>{}</{tag}>\n", spans_html(&b.spans)));
            }
            BlockKind::Paragraph { .. } => out.push_str(&format!("<p{id}>{}</p>\n", spans_html(&b.spans))),
            BlockKind::ListItem { marker } => {
                let cls = if marker.is_some() { " class=\"labeled\"" } else { "" };
                out.push_str(&format!("<li{id}{cls}>{}</li>\n", spans_html(&b.spans)));
            }
            BlockKind::Reference { .. } => out.push_str(&format!("<li{id}>{}</li>\n", spans_html(&b.spans))),
            BlockKind::Footnote { marker } => {
                let mark = if marker.is_empty() { String::new() } else { format!("<sup class=\"fn-mark\">{}</sup> ", esc(marker)) };
                out.push_str(&format!(
                    "<aside class=\"footnote\"{id} role=\"note\">{mark}{}</aside>\n",
                    spans_html(&b.spans)
                ));
            }
            BlockKind::Equation { image, number } => {
                let alt = esc(&plain(&b.spans));
                let img = match images.region(image, "eq") {
                    Some(src) => format!(
                        "<img src=\"{src}\" alt=\"{alt}\" style=\"{}\" loading=\"lazy\">",
                        ctx.unit.css(image.rect.width())
                    ),
                    None => format!("<span class=\"eq-text\">{alt}</span>"),
                };
                let num = number.as_ref().map(|n| format!("<span class=\"eqno\">{}</span>", esc(n))).unwrap_or_default();
                out.push_str(&format!(
                    "<div class=\"eq\"{id}><span class=\"eq-body\">{img}</span>{num}</div>\n"
                ));
            }
            BlockKind::Figure { kind, image, label } => {
                let cls = float_class(*kind);
                let alt = esc(&figure_alt(b, label));
                let img = image
                    .as_ref()
                    .and_then(|spec| images.region(spec, cls).map(|src| (src, spec)))
                    .map(|(src, spec)| {
                        format!(
                            "<img src=\"{src}\" alt=\"{alt}\" style=\"{}\" loading=\"lazy\">",
                            ctx.unit.css(spec.rect.width())
                        )
                    })
                    .unwrap_or_default();
                let cap = if b.spans.is_empty() {
                    String::new()
                } else {
                    format!("<figcaption>{}</figcaption>", caption_html(&b.spans))
                };
                // Keep the caption on the side it had in the paper.
                let caption_first = match (image, b.lines.first()) {
                    (Some(spec), Some(l)) => l.bbox.cy() < spec.rect.cy(),
                    _ => false,
                };
                let inner = if caption_first { format!("{cap}{img}") } else { format!("{img}{cap}") };
                out.push_str(&format!("<figure class=\"float {cls}\"{id}>{inner}</figure>\n"));
            }
        }
    }
    close(group, &mut out);
    out
}

/// Author groups as cards.
pub(crate) fn authors_html(blocks: &[&Block]) -> String {
    let cards: Vec<String> = blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Authors))
        .map(|b| {
            let rows: Vec<String> = b
                .rows
                .iter()
                .enumerate()
                .map(|(i, r)| format!("<div class=\"{}\">{}</div>", if i == 0 { "au-name" } else { "au-line" }, spans_html(r)))
                .collect();
            format!("<div class=\"author\">{}</div>", rows.join(""))
        })
        .collect();
    if cards.is_empty() {
        String::new()
    } else {
        format!("<div class=\"authors\">{}</div>\n", cards.join("\n"))
    }
}
