//! Options of the HTML output and its result.

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
