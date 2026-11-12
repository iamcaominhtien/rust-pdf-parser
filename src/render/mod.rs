//! HTML output in three fidelity modes.

mod friendly;
mod hard;
mod hard95;
mod medium;

use crate::analysis::document::Analysis;
use crate::backend::PageRenderer;
use crate::model::*;
use base64::Engine;
use hayro_syntax::Pdf;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Facsimile: every page looks exactly like the PDF, with a selectable
    /// text layer organised by paragraph.
    Hard,
    /// Like `Hard`, but everything that is text is real, visible HTML text
    /// placed where it is in the PDF; only true graphics remain pictures.
    Hard95,
    /// Page and column structure preserved, text reflowed as real HTML.
    Medium,
    /// Single-column, responsive article optimised for reading.
    Friendly,
}

impl Mode {
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Hard => "hard",
            Mode::Hard95 => "hard-95",
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
    /// Worker threads for drawing pages and images (0 = one per CPU).
    pub jobs: usize,
}

impl Default for HtmlOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Friendly,
            inline_images: true,
            asset_dir: "assets".into(),
            scale: 3.0,
            page_image: PageImage::Svg,
            jobs: 0,
        }
    }
}

pub struct HtmlOutput {
    pub html: String,
    /// Files to write next to the HTML: (relative path, contents).
    pub files: Vec<(String, Vec<u8>)>,
}

pub fn render(analysis: &Analysis, pdf: &Pdf, opts: &HtmlOptions) -> HtmlOutput {
    let mut images = ImageStore { opts, jobs: Vec::new() };
    let html = match opts.mode {
        Mode::Friendly => friendly::render(&analysis.doc, &mut images),
        Mode::Medium => medium::render(&analysis.doc, &mut images),
        Mode::Hard => hard::render(analysis, &mut images),
        Mode::Hard95 => hard95::render(analysis, &mut images),
    };
    images.finish(pdf, html)
}

/// A picture the HTML needs: a page region, or a whole page.
enum ImageJob {
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
    opts: &'r HtmlOptions,
    jobs: Vec<ImageJob>,
}

/// Marks a placeholder; a control character never written to the HTML.
const MARK: char = '\u{1}';

impl ImageStore<'_> {
    fn add(&mut self, job: ImageJob) -> String {
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
    fn draw(&self, r: &PageRenderer<'_>, job: &ImageJob) -> Option<(Vec<u8>, &'static str, &'static str, String)> {
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
    fn finish(self, pdf: &Pdf, html: String) -> HtmlOutput {
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
    spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

static CAP_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:Supplementary\s+|Extended\s+Data\s+)?(?:Figure|Fig\.?|FIGURE|FIG\.?|Table|TABLE|Tab\.|Algorithm|ALGORITHM|Listing|Scheme|Chart|Plate)\s*\S+?\s*[:.|—–-]").unwrap()
});

/// Caption HTML with the label ("Figure 3:") emphasised.
pub(crate) fn caption_html(spans: &[Span]) -> String {
    if let Some(first) = spans.first()
        && let Some(m) = CAP_LABEL.find(&first.text)
    {
        let mut rest = spans.to_vec();
        let label = first.text[..m.end()].to_string();
        rest[0].text = first.text[m.end()..].to_string();
        return format!("<span class=\"cap-label\">{}</span>{}", esc(&label), spans_html(&rest));
    }
    spans_html(spans)
}

pub(crate) fn float_class(kind: crate::analysis::floats::FloatKind) -> &'static str {
    use crate::analysis::floats::FloatKind::*;
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
pub(crate) fn render_flow(blocks: &[&Block], images: &mut ImageStore<'_>, ctx: &FlowCtx) -> String {
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
                        && plain(&blocks[bi - 1].spans)
                            .trim_end_matches(['.', ':'])
                            .eq_ignore_ascii_case("abstract");
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
                let is_abstract = plain(&b.spans)
                    .trim_end_matches(['.', ':'])
                    .eq_ignore_ascii_case("abstract");
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
                let mark = if marker.is_empty() {
                    String::new()
                } else {
                    format!("<sup class=\"fn-mark\">{}</sup> ", esc(marker))
                };
                out.push_str(&format!(
                    "<aside class=\"footnote\"{id} role=\"note\">{mark}{}</aside>\n",
                    spans_html(&b.spans)
                ));
            }
            BlockKind::Equation { image, number } => {
                let alt = esc(&plain(&b.spans));
                let img = format!(
                    "<img src=\"{}\" alt=\"{alt}\" style=\"{}\" loading=\"lazy\">",
                    images.region(image, "eq"),
                    ctx.unit.css(image.rect.width())
                );
                let num = number
                    .as_ref()
                    .map(|n| format!("<span class=\"eqno\">{}</span>", esc(n)))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "<div class=\"eq\"{id}><span class=\"eq-body\">{img}</span>{num}</div>\n"
                ));
            }
            BlockKind::Figure { kind, image, label } => {
                let cls = float_class(*kind);
                let alt = esc(&figure_alt(b, label));
                let img = image
                    .as_ref()
                    .filter(|_| b.table.is_none())
                    .map(|spec| (images.region(spec, cls), spec))
                    .map(|(src, spec)| {
                        format!(
                            "<img src=\"{src}\" alt=\"{alt}\" style=\"{}\" loading=\"lazy\">",
                            ctx.unit.css(spec.rect.width())
                        )
                    })
                    .unwrap_or_default();
                // A reconstructed table replaces the picture of the table.
                let img = match &b.table {
                    Some(t) => format!("<div class=\"tbl-wrap\">{}</div>", table_html(t)),
                    None => img,
                };
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
                let inner = if caption_first {
                    format!("{cap}{img}")
                } else {
                    format!("{img}{cap}")
                };
                out.push_str(&format!("<figure class=\"float {cls}\"{id}>{inner}</figure>\n"));
            }
        }
    }
    close(group, &mut out);
    out
}

/// Classes describing the rules around the cell at (`row`, `col`) spanning
/// `colspan` x `rowspan` positions: `ra1`/`ra2` thin/heavy rule above, `rb*`
/// below, `rl`/`rr` vertical rules.
pub(crate) fn table_rule_classes(t: &Table, row: usize, col: usize, colspan: usize, rowspan: usize) -> String {
    let n_rows = t.rows.len();
    let end = (col + colspan).min(t.n_cols);
    let last_row = (row + rowspan).min(n_rows) - 1;
    let mut cls = String::new();
    let above = t.rule_above[row][col..end].iter().copied().max().unwrap_or(0);
    if above > 0 {
        cls.push_str(&format!(" ra{above}"));
    }
    if last_row + 1 == n_rows {
        let below = t.rule_bottom[col..end].iter().copied().max().unwrap_or(0);
        if below > 0 {
            cls.push_str(&format!(" rb{below}"));
        }
    }
    if t.rule_left[row][col] {
        cls.push_str(" rl");
    }
    if end == t.n_cols && t.rule_right[row] {
        cls.push_str(" rr");
    }
    cls
}

/// Walks the cells of a table in document order, including the empty
/// positions, and calls `emit(row, col, cell)` for each.
pub(crate) fn table_walk(t: &Table, mut emit: impl FnMut(usize, usize, Option<&TableCell>)) {
    let n_rows = t.rows.len();
    // Positions covered by a row-spanning cell from an earlier row.
    let mut covered = vec![vec![false; t.n_cols]; n_rows];
    for (ri, row) in t.rows.iter().enumerate() {
        let mut c = 0;
        for cell in row {
            while c < cell.col.min(t.n_cols) {
                if !covered[ri][c] {
                    emit(ri, c, None);
                }
                c += 1;
            }
            emit(ri, cell.col, Some(cell));
            let rows_end = (ri + cell.rowspan).min(n_rows);
            let cols_end = (cell.col + cell.colspan).min(t.n_cols);
            for row_covered in covered.iter_mut().take(rows_end).skip(ri + 1) {
                for cov in row_covered.iter_mut().take(cols_end).skip(cell.col) {
                    *cov = true;
                }
            }
            c = cell.col + cell.colspan;
        }
        while c < t.n_cols {
            if !covered[ri][c] {
                emit(ri, c, None);
            }
            c += 1;
        }
    }
}

/// A reconstructed table as an HTML `<table>`.
pub(crate) fn table_html(t: &Table) -> String {
    let mut out = String::from("<table class=\"tbl\">");
    let mut open_row: Option<usize> = None;
    table_walk(t, |ri, col, cell| {
        if open_row != Some(ri) {
            if open_row.is_some() {
                out.push_str("</tr>");
            }
            if ri == 0 && t.header_rows > 0 {
                out.push_str("<thead>");
            }
            if ri == t.header_rows {
                if t.header_rows > 0 {
                    out.push_str("</thead>");
                }
                out.push_str("<tbody>");
            }
            out.push_str("<tr>");
            open_row = Some(ri);
        }
        let tag = if ri < t.header_rows { "th" } else { "td" };
        match cell {
            None => out.push_str(&format!(
                "<{tag} class=\"e{}\"></{tag}>",
                table_rule_classes(t, ri, col, 1, 1)
            )),
            Some(cell) => {
                let align = if cell.colspan > 1 {
                    "c"
                } else {
                    &t.col_align[cell.col.min(t.n_cols - 1)][..1]
                };
                let short = plain(&cell.spans).chars().count() <= 28;
                let mut attrs = String::new();
                if cell.colspan > 1 {
                    attrs.push_str(&format!(" colspan=\"{}\"", cell.colspan));
                }
                if cell.rowspan > 1 {
                    attrs.push_str(&format!(" rowspan=\"{}\"", cell.rowspan));
                }
                out.push_str(&format!(
                    "<{tag}{attrs} class=\"{align}{}{}{}\">{}</{tag}>",
                    if short { " nw" } else { "" },
                    if cell.rowspan > 1 { " mid" } else { "" },
                    table_rule_classes(t, ri, cell.col, cell.colspan, cell.rowspan),
                    spans_html(&cell.spans)
                ));
            }
        }
    });
    if open_row.is_some() {
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
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
                .map(|(i, r)| {
                    format!(
                        "<div class=\"{}\">{}</div>",
                        if i == 0 { "au-name" } else { "au-line" },
                        spans_html(r)
                    )
                })
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
