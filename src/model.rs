//! The structured document produced by the layout analysis and consumed by
//! the HTML renderers.

use crate::geom::Rect;
use crate::layout::Line;
use crate::layout::blocks::Loc;
use crate::layout::floats::FloatKind;
use serde::Serialize;

pub use crate::layout::lines::{BOLD, ITALIC, MATH, MONO, SUB, SUP};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Span {
    pub text: String,
    /// Bit set of `BOLD`, `ITALIC`, `MONO`, `MATH`, `SUP`, `SUB`.
    pub style: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
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
    pub blocks: Vec<Block>,
}
