//! The structured document produced by the layout analysis and consumed by
//! the HTML renderers.

use crate::geom::Rect;
use crate::layout::Line;
use crate::layout::blocks::Loc;
use crate::layout::floats::FloatKind;
use serde::Serialize;

pub use crate::layout::lines::{BOLD, ITALIC, MATH, MONO, SANS, SUB, SUP};
pub use crate::layout::tables::Align;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Span {
    pub text: String,
    /// Bit set of `BOLD`, `ITALIC`, `MONO`, `MATH`, `SUP`, `SUB`, `SANS`.
    pub style: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    /// Text colour as RGB; black is the default.
    #[serde(skip_serializing_if = "is_black")]
    pub color: [u8; 3],
}

fn is_black(c: &[u8; 3]) -> bool {
    *c == [0; 3]
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), style: 0, href: None, color: [0; 3] }
    }
}

/// One cell of a reconstructed table.
#[derive(Debug, Clone, Serialize)]
pub struct TableCell {
    pub col: usize,
    pub colspan: usize,
    pub rowspan: usize,
    pub spans: Vec<Span>,
}

/// A table whose rows and columns could be reconstructed.
#[derive(Debug, Clone, Serialize)]
pub struct Table {
    pub n_cols: usize,
    /// Number of leading rows that form the header.
    pub header_rows: usize,
    /// "left" / "center" / "right" per column.
    pub col_align: Vec<&'static str>,
    pub rows: Vec<Vec<TableCell>>,
    /// Rule above each cell position `[row][col]`: 0 none, 1 thin, 2 heavy.
    pub rule_above: Vec<Vec<u8>>,
    /// Rule below the last row, per column.
    pub rule_bottom: Vec<u8>,
    /// Vertical rule on the left edge of each cell position `[row][col]`.
    pub rule_left: Vec<Vec<bool>>,
    /// Vertical rule on the right edge of the table, per row.
    pub rule_right: Vec<bool>,
    /// Geometry on the page (for the position-preserving renderer).
    #[serde(skip)]
    pub geom: TableGeom,
}

#[derive(Debug, Clone, Default)]
pub struct TableGeom {
    pub bbox: Rect,
    pub col_bounds: Vec<f32>,
    pub col_text: Vec<(f32, f32)>,
    pub row_bounds: Vec<f32>,
    pub row_baselines: Vec<f32>,
    pub size: f32,
}

/// The text of one source line, as it appears in the PDF.
#[derive(Debug, Clone, Default)]
pub struct HLine {
    pub spans: Vec<Span>,
    /// The line ended with a hyphen that was removed when joining it to the
    /// next line.
    pub soft_hyphen: bool,
}

/// A run of glyphs of a displayed formula with its exact position.
#[derive(Debug, Clone)]
pub struct EqRun {
    pub text: String,
    pub x: f32,
    /// Baseline.
    pub y: f32,
    pub width: f32,
    pub size: f32,
    pub style: u8,
    pub color: [u8; 3],
    /// For glyphs that do not sit on the baseline in the usual way (big
    /// operators, tall delimiters, radicals): the box their ink occupies.
    pub ink: Option<Rect>,
}

/// A piece of rotated text (axis labels, margin stamps).
#[derive(Debug, Clone)]
pub struct RotatedRun {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub size: f32,
    pub length: f32,
    pub style: u8,
    pub color: [u8; 3],
    pub origins: Vec<(f32, f32)>,
}

/// A line of text that is not part of a text block, with its styling.
#[derive(Debug, Clone)]
pub struct PLine {
    pub line: Line,
    pub spans: Vec<Span>,
    /// "header" for running heads and page numbers, "eqno" for equation
    /// numbers.
    pub kind: &'static str,
}

/// A rectangular area of a page that is shown as an image.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ImageSpec {
    pub page: usize,
    pub rect: Rect,
    /// Render on a transparent background (equations).
    pub transparent: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    Title,
    /// One author / affiliation group; `Block::rows` holds its lines.
    Authors,
    /// Miscellaneous front-matter text (notices above the title, ...).
    Note,
    Heading {
        level: u8,
    },
    Paragraph {
        in_abstract: bool,
    },
    ListItem {
        /// `None` for bullet lists.
        marker: Option<String>,
    },
    Equation {
        image: ImageSpec,
        number: Option<String>,
    },
    Figure {
        kind: FloatKind,
        image: Option<ImageSpec>,
        label: Option<String>,
    },
    Footnote {
        marker: String,
    },
    Reference {
        label: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Block {
    pub id: usize,
    #[serde(flatten)]
    pub kind: BlockKind,
    pub page: usize,
    pub bbox: Rect,
    /// Text content. For figures this is the caption, for equations the
    /// extracted text of the formula (alt text).
    pub spans: Vec<Span>,
    /// Line-wise content for blocks whose line breaks matter (authors).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Vec<Span>>,
    /// Stable anchor for cross references ("fig-2", "sec-3.1", "ref-12").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    #[serde(skip)]
    pub loc: Loc,
    /// Source lines with their geometry (facsimile text layer).
    #[serde(skip)]
    pub lines: Vec<Line>,
    /// For each line, the text it contributes to `spans` (already joined
    /// with its successor: trailing space or removed hyphen).
    #[serde(skip)]
    pub line_texts: Vec<String>,
    /// The same, with styling.
    #[serde(skip)]
    pub hlines: Vec<HLine>,
    /// Reconstructed structure of a table float.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<Table>,
    /// Lines inside a float (labels of a figure, cells of a table) with
    /// their styled text.
    #[serde(skip)]
    pub inner_lines: Vec<Line>,
    #[serde(skip)]
    pub inner_spans: Vec<Vec<Span>>,
    /// Positioned glyph runs of an equation.
    #[serde(skip)]
    pub eq_runs: Vec<EqRun>,
}

impl Block {
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PageInfo {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Meta {
    pub title: Option<String>,
    pub arxiv_id: Option<String>,
    pub arxiv_stamp: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Document {
    pub meta: Meta,
    pub pages: Vec<PageInfo>,
    pub body_size: f32,
    pub leading: f32,
    /// PostScript name of the font of the running text.
    pub body_font: String,
    pub blocks: Vec<Block>,
}
