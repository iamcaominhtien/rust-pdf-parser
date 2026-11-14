//! Options of the HTML output and its result.

/// Fidelity mode of the HTML output: how closely it follows the look of the
/// PDF versus how readable and reflowable it is.
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
    /// The name of the mode as used on the command line and in output file
    /// names (`hard`, `hard-95`, `medium`, `friendly`).
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Hard => "hard",
            Mode::Hard95 => "hard-95",
            Mode::Medium => "medium",
            Mode::Friendly => "friendly",
        }
    }
}

/// Image format of the page backgrounds in hard mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageImage {
    /// Vector SVG: sharp at every zoom level, usually the smaller file.
    Svg,
    /// Raster PNG: for viewers that render complex SVG slowly or wrongly.
    Png,
}

/// How the reflowing modes (`medium`, `friendly`) show displayed formulas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Equations {
    /// A picture of the formula's region of the page.
    Image,
    /// MathML rebuilt from the glyphs, when the structure of the formula is
    /// clear; a picture otherwise.
    MathMl,
}

/// Everything that controls how a document is written as HTML.
#[derive(Debug, Clone)]
pub struct HtmlOptions {
    /// Fidelity mode of the output.
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
    /// Embed the reading fonts in friendly-mode pages, and the math font in
    /// medium and friendly pages with MathML formulas (otherwise the
    /// reader's system fonts are used).
    pub embed_fonts: bool,
    /// How displayed formulas are shown in the reflowing modes.
    pub equations: Equations,
}

impl Default for HtmlOptions {
    /// Friendly mode, one self-contained file with embedded fonts, SVG page
    /// backgrounds, images at 3 pixels per point, formulas as MathML (where
    /// their structure is clear) and one worker thread per CPU.
    fn default() -> Self {
        Self {
            mode: Mode::Friendly,
            inline_images: true,
            asset_dir: "assets".into(),
            scale: 3.0,
            page_image: PageImage::Svg,
            jobs: 0,
            embed_fonts: true,
            equations: Equations::MathMl,
        }
    }
}

/// The result of the HTML output: the page itself and, when images are not
/// inlined, the image files it refers to.
pub struct HtmlOutput {
    /// The complete HTML document.
    pub html: String,
    /// Files to write next to the HTML: (relative path, contents).
    pub files: Vec<(String, Vec<u8>)>,
}
