//! Assembling the final [`Document`] from the classified nodes.
//!
//! Every node becomes one output [`Block`] with its styled text and a
//! unique anchor where it can be referenced (headings, references,
//! footnotes, equations, floats). Once all blocks exist, cross-references
//! and footnote marks in the running text are linked to those anchors.

use super::links::*;
use super::patterns::*;
use super::text::*;
use super::*;
use crate::analysis::blocks::LIST_LABEL;
use crate::analysis::lines::split_at_gaps;
use crate::analysis::page::{DocStats, PageData};
use crate::analysis::tables::extract_grid;
use crate::backend::FontInfo;
use crate::common::geom::Rect;
use crate::config::AnalyzeOptions;
use crate::model::FloatKind;
use crate::model::Line;
use crate::model::Loc;
use crate::model::*;
use std::collections::{HashMap, HashSet};

/// Value of [`LineGlyph::script`] for a superscript glyph.
const SUPERSCRIPT: i8 = 1;

/// Tuning parameters of document assembly. Part of [`SemanticsConfig`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AssembleConfig {
    /// Author lines whose baselines differ by less than this (points) are
    /// output as one row, their pieces separated by an em space.
    pub author_row_baseline_tolerance_pt: f32,
    /// Margin (points) added around an equation's box for its image, so
    /// that the ink of the glyphs is not clipped.
    pub equation_margin_pt: f32,
    /// Lines inside a table are split at gaps wider than this (multiple of
    /// the font size) before the grid is reconstructed: cells can be closer
    /// together than the gap at which lines are broken.
    pub table_cell_gap_em: f32,
    /// A superscript after a word of fewer letters than this is an exponent
    /// (x², c²), not a footnote mark.
    pub mark_min_word_letters_count: usize,
}

impl Default for AssembleConfig {
    fn default() -> Self {
        Self {
            author_row_baseline_tolerance_pt: 2.0,
            equation_margin_pt: 2.5,
            table_cell_gap_em: 0.55,
            mark_min_word_letters_count: 2,
        }
    }
}

/// The blocks being built, with the state shared between them.
struct Assembler<'a> {
    /// Builds the styled text.
    text: &'a TextBuilder<'a>,
    /// The pages, for clipping equation images.
    pages: &'a [PageData],
    /// Analysis options (table reconstruction and its thresholds).
    opts: &'a AnalyzeOptions,
    /// Thresholds of the assembly.
    cfg: &'a AssembleConfig,
    /// The blocks built so far; a block's id is its index.
    blocks: Vec<Block>,
    /// Every anchor given out so far.
    anchors: HashSet<String>,
    /// Lines outside every block, per page (running heads, equation numbers).
    loose: Vec<Vec<PLine>>,
    /// Text of each title block.
    title_parts: Vec<String>,
    /// Number of unnumbered equations so far, for their anchors.
    eq_counter: usize,
}

/// What a text node contributes to its block besides its lines.
struct TextContent {
    /// Kind of the output block.
    kind: BlockKind,
    /// Styled text.
    built: Built,
    /// Row-wise text (authors only).
    rows: Vec<Vec<Span>>,
    /// Anchor of the block, if it can be referenced.
    anchor: Option<String>,
}

/// Builds the document from the classified nodes, in their order.
///
/// `furniture` holds the running heads and page numbers removed from each
/// page; they are returned, with the equation numbers, as the loose lines
/// of each page (second element of the result). The document title is
/// taken from the title blocks when there are any.
pub(crate) fn build_document(
    nodes: Vec<Node>,
    pages: &[PageData],
    stats: &DocStats,
    fonts: &[FontInfo],
    mut meta: Meta,
    furniture: Vec<Vec<Line>>,
    opts: AnalyzeOptions,
) -> (Document, Vec<Vec<PLine>>) {
    let vocab = build_vocab(nodes.iter().filter_map(|node| match &node.kind {
        NodeKind::Text(block) | NodeKind::Footnote(block) => Some(block.lines.as_slice()),
        NodeKind::Float(float) => Some(float.caption.as_slice()),
        _ => None,
    }));
    let text = TextBuilder {
        fonts,
        vocab: &vocab,
        cfg: &opts.layout.semantics.text,
    };
    let mut assembler = Assembler {
        text: &text,
        pages,
        opts: &opts,
        cfg: &opts.layout.semantics.assemble,
        blocks: Vec::new(),
        anchors: HashSet::new(),
        loose: furniture_lines(furniture, pages.len(), &text),
        title_parts: Vec::new(),
        eq_counter: 0,
    };
    for node in nodes {
        assembler.push_node(node);
    }
    let Assembler {
        mut blocks,
        anchors,
        loose,
        title_parts,
        cfg,
        ..
    } = assembler;
    link_blocks(&mut blocks, &anchors, cfg);

    if !title_parts.is_empty() {
        meta.title = Some(title_parts.join(" ").split_whitespace().collect::<Vec<_>>().join(" "));
    }
    let doc = Document {
        meta,
        pages: pages
            .iter()
            .map(|page| PageInfo {
                width: page.width,
                height: page.height,
            })
            .collect(),
        body_size: stats.body_size,
        leading: stats.leading,
        body_font: fonts
            .get(stats.body_font as usize)
            .map(|font| font.name.clone())
            .unwrap_or_default(),
        blocks,
    };
    (doc, loose)
}

/// Whether a glyph's text is one of the symbols used as footnote marks
/// (asterisks, daggers, section and pilcrow signs).
pub(crate) fn is_footnote_symbol(text: &str) -> bool {
    matches!(text, "∗" | "†" | "‡" | "§" | "¶" | "*")
}

impl Assembler<'_> {
    /// Builds the block of one node and appends it.
    fn push_node(&mut self, node: Node) {
        let id = self.blocks.len();
        let loc = node.loc;
        let block = match node.kind {
            NodeKind::Text(text_block) => {
                let bbox = lines_bbox(&text_block.lines);
                let content = self.text_content(node.class, &text_block.lines, node.strip_prefix);
                let mut block = block_with_text(id, content.kind, loc, bbox, content.built);
                block.rows = content.rows;
                block.anchor = content.anchor;
                block.lines = text_block.lines;
                block
            }
            NodeKind::Footnote(text_block) => self.footnote_block(id, loc, text_block),
            NodeKind::Equation(equation) => self.equation_block(id, loc, equation),
            NodeKind::Float(float) => self.float_block(id, loc, float),
        };
        self.blocks.push(block);
    }

    /// Returns `base` if it is still free, else the first free `base-2`,
    /// `base-3`, …, and reserves it.
    fn unique_anchor(&mut self, base: String) -> String {
        let mut anchor = base.clone();
        let mut k = 2;
        while !self.anchors.insert(anchor.clone()) {
            anchor = format!("{base}-{k}");
            k += 1;
        }
        anchor
    }

    /// Kind, text and anchor of a text node of class `class`.
    /// `strip_prefix` characters (an inline label) are dropped from the
    /// text of paragraphs.
    fn text_content(&mut self, class: Class, lines: &[Line], strip_prefix: usize) -> TextContent {
        let mut rows = Vec::new();
        let mut anchor = None;
        let (kind, built) = match class {
            Class::Title => {
                let built = self.text.build(lines, 0, BOLD);
                self.title_parts.push(spans_text(&built.spans));
                (BlockKind::Title, built)
            }
            Class::Authors => {
                rows = self.author_rows(lines);
                (BlockKind::Authors, self.text.build(lines, 0, 0))
            }
            Class::Note => (BlockKind::Note, self.text.build(lines, 0, 0)),
            Class::Heading { level, number } => {
                let built = self.text.build(lines, 0, BOLD);
                let base = match &number {
                    Some(num) => format!("sec-{num}"),
                    None => format!("sec-{}", slug(&spans_text(&built.spans))),
                };
                anchor = Some(self.unique_anchor(base));
                (BlockKind::Heading { level }, built)
            }
            Class::ListItem { bullet } => {
                let built = self.text.build(lines, if bullet { 1 } else { 0 }, 0);
                let marker = if bullet {
                    None
                } else {
                    let text = spans_text(&built.spans);
                    LIST_LABEL.find(&text).map(|label| label.as_str().trim().to_string())
                };
                (BlockKind::ListItem { marker }, built)
            }
            Class::Reference => {
                let built = self.text.build(lines, 0, 0);
                let label = reference_label(&spans_text(&built.spans));
                if let Some(label) = &label {
                    anchor = Some(self.unique_anchor(format!("ref-{}", slug(label))));
                }
                (BlockKind::Reference { label }, built)
            }
            Class::Paragraph { in_abstract } => {
                let mut built = self.text.build(lines, 0, 0);
                if strip_prefix > 0 {
                    strip_chars(&mut built.spans, strip_prefix);
                }
                (BlockKind::Paragraph { in_abstract }, built)
            }
            Class::Unknown => (
                BlockKind::Paragraph { in_abstract: false },
                self.text.build(lines, 0, 0),
            ),
        };
        TextContent {
            kind,
            built,
            rows,
            anchor,
        }
    }

    /// Rows of an author card: one per baseline; side-by-side pieces are
    /// separated by an em space.
    fn author_rows(&self, lines: &[Line]) -> Vec<Vec<Span>> {
        let mut rows: Vec<Vec<Span>> = Vec::new();
        let mut last_baseline = f32::NEG_INFINITY;
        for line in lines {
            let mut spans = self.text.line_spans(line);
            if (line.baseline - last_baseline).abs() < self.cfg.author_row_baseline_tolerance_pt && !rows.is_empty() {
                let row: &mut Vec<Span> = rows.last_mut().unwrap();
                row.push(Span::plain("\u{2003}"));
                row.append(&mut spans);
            } else {
                rows.push(spans);
            }
            last_baseline = line.baseline;
        }
        rows
    }

    /// The block of a footnote. Its mark (leading superscript glyphs, or a
    /// leading footnote symbol) is taken out of the text into the block
    /// kind and gives the anchor.
    fn footnote_block(&mut self, id: usize, loc: Loc, text_block: TextBlock) -> Block {
        let bbox = lines_bbox(&text_block.lines);
        let first = &text_block.lines[0];
        let mut skip = first
            .glyphs
            .iter()
            .take_while(|glyph| glyph.script == SUPERSCRIPT)
            .count();
        if skip == 0
            && first
                .glyphs
                .first()
                .is_some_and(|glyph| is_footnote_symbol(&glyph.text))
        {
            skip = 1;
        }
        let marker: String = first.glyphs[..skip].iter().map(|glyph| glyph.text.as_str()).collect();
        let built = self.text.build(&text_block.lines, skip, 0);
        let anchor = (!marker.is_empty()).then(|| self.unique_anchor(format!("fn-{}", slug_marker(&marker))));
        let mut block = block_with_text(id, BlockKind::Footnote { marker }, loc, bbox, built);
        block.anchor = anchor;
        block.lines = text_block.lines;
        block
    }

    /// The block of a displayed formula: an image of its area, its text as
    /// alt text and positioned glyph runs. Numbered equations are anchored
    /// by their number, the others by a running count; the equation number
    /// becomes a loose line of the page.
    fn equation_block(&mut self, id: usize, loc: Loc, equation: EquationBlock) -> Block {
        let mut lines = equation.lines;
        lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
        let alt = lines.iter().map(|line| line.text()).collect::<Vec<_>>().join(" ");
        let margin = self.cfg.equation_margin_pt;
        let rect = equation
            .bbox
            .inflate(margin, margin)
            .intersect(&self.pages[loc.page].rect());
        let anchor = match &equation.number {
            Some(num) => Some(self.unique_anchor(format!("eq-{}", num.trim_matches(['(', ')'])))),
            None => {
                self.eq_counter += 1;
                Some(self.unique_anchor(format!("eq-u{}", self.eq_counter)))
            }
        };
        if let Some(line) = equation.number_line {
            let spans = self.text.line_spans(&line);
            self.loose[loc.page].push(PLine {
                line,
                spans,
                kind: "eqno",
            });
        }
        let image = ImageSpec {
            page: loc.page,
            rect,
            transparent: true,
        };
        let kind = BlockKind::Equation {
            image,
            number: equation.number,
        };
        let mut block = new_block(id, kind, loc, rect);
        block.spans = vec![Span::plain(alt)];
        block.line_texts = lines.iter().map(|line| line.text()).collect();
        block.eq_runs = self.text.eq_runs(&lines);
        block.anchor = anchor;
        block.lines = lines;
        block
    }

    /// The block of a figure, table or algorithm: its caption as text, an
    /// image of its region, the text inside it and, for simple tables (when
    /// enabled), the reconstructed table. Labelled floats are anchored by
    /// their number.
    fn float_block(&mut self, id: usize, loc: Loc, float: Float) -> Block {
        let built = self.text.build(&float.caption, 0, 0);
        let anchor = float.label.as_ref().map(|label| {
            let num = label.rsplit(' ').next().unwrap_or("");
            let prefix = match float.kind {
                FloatKind::Table => "tab",
                FloatKind::Algorithm => "alg",
                _ => "fig",
            };
            self.unique_anchor(format!("{prefix}-{num}"))
        });
        let mut rows = Vec::new();
        if !float.inner_text.is_empty() {
            rows.push(vec![Span::plain(float.inner_text.clone())]);
        }
        let table = self.float_table(&float);
        let kind = BlockKind::Figure {
            kind: float.kind,
            image: float.region.map(|rect| ImageSpec {
                page: float.page,
                rect,
                transparent: false,
            }),
            label: float.label.clone(),
        };
        let mut block = block_with_text(id, kind, loc, float.bbox, built);
        block.rows = rows;
        block.anchor = anchor;
        block.lines = float.caption;
        block.inner_spans = float
            .inner_lines
            .iter()
            .map(|line| self.text.line_spans(line))
            .collect();
        block.inner_lines = float.inner_lines;
        block.table = table;
        block
    }

    /// The reconstructed grid of a tabular float, if table reconstruction is
    /// enabled, the float has a region, looks tabular and holds no images;
    /// `None` also when no grid is found.
    fn float_table(&self, float: &Float) -> Option<Table> {
        let tabular = float.kind == FloatKind::Table || (float.kind == FloatKind::Graphic && float.table_like);
        match float.region {
            Some(region) if self.opts.tables && tabular && !float.complex => {
                // Cells can be closer together than the gap at which
                // lines are broken; split more finely. Pieces that
                // end up in the same column are joined again.
                let pieces: Vec<Line> = float
                    .inner_lines
                    .iter()
                    .flat_map(|line| {
                        split_at_gaps(line, self.cfg.table_cell_gap_em * line.size, &self.opts.layout.lines)
                    })
                    .collect();
                extract_grid(region, &pieces, &float.hrules, &float.vrules, &self.opts.layout.tables)
                    .map(|grid| self.text.table(grid))
            }
            _ => None,
        }
    }
}

/// The removed running heads and page numbers as loose lines, one list per
/// page (`n_pages` lists).
fn furniture_lines(furniture: Vec<Vec<Line>>, n_pages: usize, text: &TextBuilder<'_>) -> Vec<Vec<PLine>> {
    let mut loose: Vec<Vec<PLine>> = vec![Vec::new(); n_pages];
    for (page_idx, lines) in furniture.into_iter().enumerate() {
        for line in lines {
            let spans = text.line_spans(&line);
            loose[page_idx].push(PLine {
                line,
                spans,
                kind: "header",
            });
        }
    }
    loose
}

/// An empty block of the given kind, place and box; the caller fills in the
/// content.
fn new_block(id: usize, kind: BlockKind, loc: Loc, bbox: Rect) -> Block {
    Block {
        id,
        kind,
        page: loc.page,
        bbox,
        spans: Vec::new(),
        rows: Vec::new(),
        anchor: None,
        loc,
        lines: Vec::new(),
        line_texts: Vec::new(),
        hlines: Vec::new(),
        table: None,
        inner_lines: Vec::new(),
        inner_spans: Vec::new(),
        eq_runs: Vec::new(),
    }
}

/// A block with the styled text `built`.
fn block_with_text(id: usize, kind: BlockKind, loc: Loc, bbox: Rect, built: Built) -> Block {
    let mut block = new_block(id, kind, loc, bbox);
    block.spans = built.spans;
    block.line_texts = built.line_texts;
    block.hlines = built.hlines;
    block
}

/// Union of the boxes of the lines.
fn lines_bbox(lines: &[Line]) -> Rect {
    lines.iter().fold(Rect::EMPTY, |bbox, line| bbox.union(&line.bbox))
}

/// Concatenated text of spans.
fn spans_text(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// The label of a reference entry ("12" of `[12]` or "12."), if it has one.
fn reference_label(text: &str) -> Option<String> {
    REF_LABEL
        .captures(text.trim())
        .map(|captures| captures.name("a").or(captures.name("b")).unwrap().as_str().to_string())
}

/// Anchor-safe form of a footnote mark: symbols are spelled out, letters
/// and digits kept, anything else replaced by "x".
fn slug_marker(marker: &str) -> String {
    marker
        .chars()
        .map(|ch| match ch {
            '∗' | '*' => "star".to_string(),
            '†' => "dagger".to_string(),
            '‡' => "ddagger".to_string(),
            '§' => "sect".to_string(),
            '¶' => "para".to_string(),
            ch if ch.is_alphanumeric() => ch.to_string(),
            _ => "x".to_string(),
        })
        .collect()
}

/// Links the cross-references in every block (except equations, titles and
/// headings) and the footnote marks in every non-footnote block.
fn link_blocks(blocks: &mut [Block], anchors: &HashSet<String>, cfg: &AssembleConfig) {
    // Footnote marks in the running text.
    let fn_markers: HashMap<String, String> = blocks
        .iter()
        .filter_map(|block| match (&block.kind, &block.anchor) {
            (BlockKind::Footnote { marker }, Some(anchor)) if marker.chars().all(|ch| ch.is_ascii_digit()) => {
                Some((marker.clone(), anchor.clone()))
            }
            _ => None,
        })
        .collect();

    for block in blocks.iter_mut() {
        let linkable = !matches!(
            block.kind,
            BlockKind::Equation { .. } | BlockKind::Title | BlockKind::Heading { .. }
        );
        if linkable {
            link_cross_references(block, anchors);
        }
        if !matches!(block.kind, BlockKind::Footnote { .. }) {
            link_footnote_marks(&mut block.spans, &fn_markers, cfg);
        }
    }
}

/// Links the cross-references in the running text, rows, line spans and
/// table cells of a block. References to the block's own anchor are not
/// linked in the running and line text.
fn link_cross_references(block: &mut Block, anchors: &HashSet<String>) {
    let own = block.anchor.clone();
    let spans = std::mem::take(&mut block.spans);
    block.spans = link_spans(spans, anchors, own.as_deref());
    let rows = std::mem::take(&mut block.rows);
    block.rows = rows.into_iter().map(|row| link_spans(row, anchors, None)).collect();
    for hline in block.hlines.iter_mut() {
        let spans = std::mem::take(&mut hline.spans);
        hline.spans = link_spans(spans, anchors, own.as_deref());
    }
    if let Some(table) = block.table.as_mut() {
        for cell in table.rows.iter_mut().flatten() {
            let spans = std::mem::take(&mut cell.spans);
            cell.spans = link_spans(spans, anchors, None);
        }
    }
}

/// Links superscript numbers in the running text to the footnote with that
/// mark (`fn_markers`: mark → anchor), unless they look like exponents.
fn link_footnote_marks(spans: &mut [Span], fn_markers: &HashMap<String, String>, cfg: &AssembleConfig) {
    for i in 0..spans.len() {
        let span = &spans[i];
        if span.style & SUP != 0 && span.style & MATH == 0 && span.href.is_none() {
            let exponent = is_exponent(spans, i, cfg);
            if let (Some(anchor), false) = (fn_markers.get(span.text.trim()), exponent) {
                spans[i].href = Some(format!("#{anchor}"));
            }
        }
    }
}

/// Whether the superscript span at `i` is an exponent rather than a
/// footnote mark. A footnote mark follows a word or punctuation; a digit or
/// a single letter before it means an exponent (x², c²), and so does
/// nothing or another superscript before it.
fn is_exponent(spans: &[Span], i: usize, cfg: &AssembleConfig) -> bool {
    i == 0 || spans[i - 1].style & SUP != 0 || {
        let prev = spans[i - 1].text.trim_end();
        let run = prev.chars().rev().take_while(|ch| ch.is_alphabetic()).count();
        match prev.chars().last() {
            Some(ch) if ch.is_ascii_digit() => true,
            Some(ch) if ch.is_alphabetic() => run < cfg.mark_min_word_letters_count,
            Some(_) => false,
            None => true,
        }
    }
}
