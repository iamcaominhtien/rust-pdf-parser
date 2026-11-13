//! Detection of figures, tables and other graphics, and their captions.
//!
//! Vector drawings and images are clustered into components, captions are
//! found by their label ("Figure 3:"), and each caption claims the nearest
//! component. Everything inside a claimed region is rendered as an image
//! later, so the text inside it is removed from the text flow.

use super::lines::LinesConfig;
use super::page::{DocStats, PageData};
use crate::backend::FontInfo;
use crate::common::geom::Rect;
use crate::model::FloatKind;
use crate::model::{Line, LineGlyph};
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Extra cells added to each dimension of the clustering grid, so that the
/// dilated box of an element touching the page edge still has a cell to fall
/// into instead of being clamped onto the content.
const GRID_PADDING_CELLS: usize = 2;

/// Component label of a grid cell that no flood fill has reached yet.
const UNLABELLED: u32 = u32::MAX;

/// A caption label at the start of a line: the float kind ("Figure",
/// "Table", "Algorithm", ...), its number ("3", "IV", "A.2", "S1") and the
/// punctuation that follows it, if any. A label followed by punctuation or
/// standing alone is a caption; a bare "Fig. 3 shows" is a reference.
static CAPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<kind>Supplementary\s+Figure|Supplementary\s+Table|Extended\s+Data\s+Fig\.?|Extended\s+Data\s+Table|Figure|Fig\.?|FIGURE|FIG\.?|Table|TABLE|Tab\.|Algorithm|ALGORITHM|Listing|Scheme|Chart|Plate)\s*(?P<num>\d+[a-z]?|[IVXLC]+|[A-Z]\.?\d+|S\d+)\s*(?P<punct>[:.|—–-]|$)?",
    )
    .unwrap()
});

/// A numbered section heading ("3.2 Results", "IV. Method", "A. Proofs"):
/// such a line next to a drawing is never absorbed as a sub-figure title.
static HEADING_LIKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\d{1,2}(?:\.\d{1,2})*\.?|[IVX]+\.|[A-Z]\.)\s+\p{Lu}").unwrap());

/// Tuning parameters of float detection. Part of [`LayoutConfig`](crate::config::LayoutConfig).
///
/// Lengths are in PDF points (`_pt`) or in multiples of a font size (`_em`,
/// of the line's own size unless stated otherwise); `_ratio` fields are
/// fractions between 0 and 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatsConfig {
    // --- running text ---
    /// A line with at least this many characters may be running prose
    /// rather than a label inside a figure. Also used to find the left
    /// margin of the running text and to keep long lines out of the
    /// sub-figure labels.
    pub body_min_chars: usize,
    /// A prose line is also wider than this many body font sizes: about a
    /// column of text, much wider than any axis label or table cell.
    pub body_min_width_em: f32,

    // --- captions ---
    /// The label of a caption counts as bold when at least this many tenths
    /// of its glyphs are set in a bold font (and fewer than half of the
    /// glyphs of the rest are).
    pub caption_label_bold_tenths: usize,
    /// How many lines above a caption candidate are searched for a
    /// paragraph it would continue.
    pub caption_lookback_count: usize,
    /// A line above the candidate continues into it when its baseline is
    /// more than this many sizes above (so it is not on the same row)...
    pub continuation_min_dy_em: f32,
    /// ... and less than this many sizes above: normal leading of a
    /// paragraph.
    pub continuation_max_dy_em: f32,
    /// ... and its font size differs by less than this.
    pub continuation_size_tol_pt: f32,
    /// ... and it overlaps the candidate horizontally by more than this
    /// fraction of the narrower of the two lines.
    pub continuation_overlap_ratio: f32,
    /// ... and it has more than this many characters (a real text line, not
    /// a stray label).
    pub continuation_min_chars: usize,
    /// The second line of a caption starts at most this many sizes below
    /// the first, or at the body's leading (times `caption_step_max_leading`,
    /// plus `caption_step_slack_pt`): small captions set at the body's
    /// leading (ACL) are further apart than their size suggests.
    pub caption_first_step_max_em: f32,
    /// Further caption lines follow at most this multiple of the leading
    /// measured between the first two lines...
    pub caption_step_max_leading: f32,
    /// ... plus this slack.
    pub caption_step_slack_pt: f32,
    /// A caption line is at least this many sizes below the previous one;
    /// closer lines are on the same row (sub- and superscripts).
    pub caption_step_min_em: f32,
    /// Caption lines differ in font size by at most this.
    pub caption_size_tol_pt: f32,
    /// A caption line may start at most this many sizes left of the caption
    /// so far (hanging indentation of the label).
    pub caption_left_overhang_em: f32,
    /// A caption line may end at most this far right of the caption so far.
    pub caption_right_overhang_pt: f32,
    /// A line that starts with its own punctuated label within this
    /// distance of the caption's left edge starts the next caption.
    pub caption_next_label_tol_pt: f32,
    /// Width of the space before a word, in sizes, when testing whether the
    /// next word would have fit on the previous line.
    pub caption_word_space_em: f32,
    /// A previous line that ends earlier than this before the caption's
    /// right edge although the next word would have fit ends the caption.
    pub caption_short_line_slack_pt: f32,
    /// The caption stops growing after more than this many lines; guards
    /// against swallowing a whole column.
    pub caption_max_lines_count: usize,
    /// A horizontal rule at least this wide (in points) that runs between two
    /// lines of text and under the lower one ends a caption: the lower line
    /// is the header row of the table, not more caption.
    pub caption_rule_min_width_pt: f32,
    /// A rule separates two caption lines only if it lies this far (in font
    /// sizes) above the baseline of the lower one: at least the height of
    /// its lowercase letters.
    pub caption_rule_clear_em: f32,

    // --- joining bare labels ---
    /// A bare label and its text are on the same baseline within this.
    pub label_join_baseline_tol_pt: f32,
    /// A bare label and its text differ in font size by less than this.
    pub label_join_size_tol_pt: f32,
    /// The text starts less than this many sizes right of the label.
    pub label_join_max_gap_em: f32,

    // --- clustering of graphics ---
    /// Side of a cell of the clustering grid. Smaller cells separate
    /// drawings that are close together, at the cost of a larger grid.
    pub grid_cell_pt: f32,
    /// Every element is grown by this much before it is drawn into the
    /// grid, so that elements this close together form one component.
    pub cluster_dilate_pt: f32,
    /// Images covering more than this fraction of the page are page
    /// backgrounds (scans, watermarks) and are ignored.
    pub background_image_area_ratio: f32,
    /// Paths spanning more than this fraction of the page in both
    /// directions are page frames or backgrounds and are ignored.
    pub background_path_extent_ratio: f32,
    /// Paths with an alpha value below this are invisible and ignored.
    pub min_path_alpha: u8,
    /// A component needs at least this many paths to be a ruled table.
    pub ruled_min_paths_count: usize,
    /// A component is a ruled table when at least this many tenths of its
    /// paths are straight rules (and it has no image).
    pub ruled_rule_tenths: usize,

    // --- tables made of horizontal rules only (booktabs) ---
    /// Horizontal rules shorter than this are not table rules.
    pub booktabs_min_rule_width_pt: f32,
    /// Pages with more rules than this are not searched (the grouping is
    /// quadratic, and such pages are drawings, not tables).
    pub booktabs_max_rules_count: usize,
    /// Rules of one table start and end within this of each other.
    pub booktabs_extent_tol_pt: f32,
    /// A group of rules lower than this is not a table.
    pub booktabs_min_height_pt: f32,
    /// A caption between two rules splits the group when it overlaps the
    /// upper rule by more than this fraction of the narrower of the two.
    pub booktabs_caption_overlap_ratio: f32,
    /// Two consecutive rules farther apart than this belong to different
    /// tables.
    pub booktabs_max_rule_gap_pt: f32,

    // --- candidate components ---
    /// Components narrower or lower than this are ignored (bullets, rules
    /// under a word, logos).
    pub min_component_pt: f32,
    /// A component covering more than this fraction of the page ...
    pub page_sized_area_ratio: f32,
    /// ... on a page with more than this many lines is a page decoration,
    /// not a float.
    pub page_sized_min_lines_count: usize,
    /// A rule table within this distance of a drawing component is part of
    /// it (or contains it).
    pub rule_table_containment_tol_pt: f32,

    // --- votes for the table caption convention ---
    /// A table is next to its caption when it overlaps the caption
    /// horizontally by at least this fraction of the narrower of the two...
    pub vote_overlap_ratio: f32,
    /// ... and the vertical gap between them is at least this (negative:
    /// a slight overlap is allowed) ...
    pub vote_min_gap_pt: f32,
    /// ... and at most this.
    pub vote_max_gap_pt: f32,

    // --- pairing captions and components ---
    /// A caption can claim a component that overlaps it horizontally by at
    /// least this fraction of the narrower of the two.
    pub pair_overlap_ratio: f32,
    /// A component lies above (below) a caption when it ends (starts) within
    /// this of the caption's top (bottom) edge.
    pub side_tol_pt: f32,
    /// A caption only claims components at most this far away.
    pub pair_max_dist_pt: f32,
    /// A prose line lies between a caption and a component when it overlaps
    /// the caption by more than this fraction of its own width.
    pub body_between_overlap_ratio: f32,
    /// Score penalty for a figure below its caption (figure captions are
    /// usually set below the figure).
    pub penalty_figure_below_pt: f32,
    /// Score penalty for a table on the side of its caption that contradicts
    /// the document's convention.
    pub penalty_table_against_convention_pt: f32,
    /// Score penalty for a table above its caption when the document has no
    /// clear convention (table captions are more often above).
    pub penalty_table_above_pt: f32,
    /// Score penalty for an algorithm above its caption.
    pub penalty_algorithm_above_pt: f32,
    /// Score penalty for a figure caption claiming a ruled table.
    pub penalty_figure_on_table_pt: f32,
    /// Score penalty for a table caption claiming a drawing that is not a
    /// ruled table.
    pub penalty_table_on_drawing_pt: f32,
    /// Score penalty for a caption whose centre is off the component's
    /// centre by the full width of the wider of the two.
    pub misalign_weight_pt: f32,

    // --- growing caption regions ---
    /// An unclaimed component within this distance of a region joins it.
    pub grow_touch_pt: f32,
    /// A component stacked on a region joins it when it overlaps the region
    /// horizontally by more than this fraction of the narrower of the two...
    pub grow_vertical_overlap_ratio: f32,
    /// ... and the vertical gap is at most this (and no prose in between).
    pub grow_max_vgap_pt: f32,
    /// A component next to a region joins it when it lies within the
    /// caption's extent widened by this on both sides...
    pub grow_caption_margin_pt: f32,
    /// ... and it overlaps the region vertically by more than this fraction
    /// of the lower of the two...
    pub grow_horizontal_overlap_ratio: f32,
    /// ... or, for a one-line caption, the horizontal gap is at most this.
    pub grow_max_hgap_pt: f32,

    // --- uncaptioned graphics ---
    /// An unclaimed component with an image is kept as a graphic when it is
    /// at least this wide and high.
    pub graphic_image_min_pt: f32,
    /// An unclaimed drawing is kept when it has at least this many paths...
    pub graphic_min_paths_count: usize,
    /// ... and is at least this wide ...
    pub graphic_min_width_pt: f32,
    /// ... and at least this high.
    pub graphic_min_height_pt: f32,
    /// An unclaimed ruled table is kept when it has at least this many
    /// rules.
    pub graphic_min_rules_count: usize,
    /// Uncaptioned graphics within this distance of each other are merged.
    pub graphic_merge_pt: f32,
    /// A horizontal rule whose middle lies within a text line, at least this
    /// fraction of the line's height away from its top and bottom, and with
    /// text of that line beside it, is part of the text (a fraction bar, an
    /// overline): not a table rule or a drawing.
    pub inline_rule_band_ratio: f32,
    /// ... or a piece of the formula is within this many font sizes of
    /// the bar's end.
    pub inline_rule_max_gap_em: f32,

    // --- tables without graphics ---
    /// A line with at least this many characters ...
    pub fallback_prose_min_chars: usize,
    /// ... that is at least this fraction of the caption's width ...
    pub fallback_prose_width_ratio: f32,
    /// ... (with the caption counted as at least this wide) is prose and
    /// ends the table rows.
    pub fallback_prose_min_width_pt: f32,
    /// Table rows overlap the caption widened by this on both sides.
    pub fallback_x_margin_pt: f32,
    /// A caption whose centre is further than this fraction of the page
    /// width from the page's centre is in one column; its table's rows are
    /// looked for within that column only.
    pub fallback_column_offset_ratio: f32,
    /// Table rows may overlap the caption vertically by this much.
    pub fallback_edge_tol_pt: f32,
    /// A gap between rows larger than this many body sizes ends the table.
    pub fallback_max_row_gap_em: f32,
    /// At least this many rows make a table.
    pub fallback_min_rows_count: usize,
    /// The rows must span more than this many body sizes.
    pub fallback_min_height_em: f32,

    // --- absorbing text into a region ---
    /// Lines touching the region are absorbed over at most this many
    /// passes (each pass may grow the region).
    pub absorb_max_passes_count: usize,
    /// A line touches the region when it overlaps the region grown by this.
    pub absorb_probe_pt: f32,
    /// A line inside the region grown by this is inside it.
    pub absorb_inside_tol_pt: f32,
    /// A touching line overlaps the probe vertically by at least this
    /// fraction of its height...
    pub absorb_touch_y_ratio: f32,
    /// ... and horizontally by at least this fraction of its width (or has
    /// its centre in the probe).
    pub absorb_touch_x_ratio: f32,

    // --- sub-figure labels around a drawing ---
    /// A label may overlap the drawing by up to this much (negative gap).
    pub label_min_gap_pt: f32,
    /// A label above or below the drawing is at most this far from it.
    pub label_max_vgap_pt: f32,
    /// A label left or right of the drawing is at most this far from it.
    pub label_max_hgap_pt: f32,
    /// A label overlaps the drawing by at least this fraction of its own
    /// width (stacked) or height (beside).
    pub label_overlap_ratio: f32,
    /// A label beside the drawing has fewer than this many characters.
    pub side_label_max_chars: usize,
    /// A bold line is the drawing's title, not a heading, when its centre
    /// is within this fraction of the drawing's width of the drawing's
    /// centre...
    pub title_center_tol_ratio: f32,
    /// ... (with the drawing counted as at least this wide) ...
    pub title_min_width_pt: f32,
    /// ... and it starts more than this right of the text margin.
    pub title_min_indent_pt: f32,
    /// ... or starts within this distance (in points) of the drawing's left
    /// edge (a chart title set flush left).
    pub title_edge_tol_pt: f32,
    /// Labels around a drawing are looked for in this many passes, each
    /// against the drawing grown by the labels found before.
    pub label_max_passes_count: usize,
    /// A heading is at least the body size minus this.
    pub heading_size_tol_pt: f32,
    /// A bold line with at least this many characters may be a heading.
    pub heading_min_chars: usize,
    /// A line with at least this fraction of bold glyphs is bold.
    pub heading_bold_ratio: f32,
    /// A label is part of a paragraph when another line of a font size
    /// within this ...
    pub attached_size_tol_pt: f32,
    /// ... is more than this many sizes above or below it ...
    pub attached_min_dy_em: f32,
    /// ... and less than this many (normal leading) ...
    pub attached_max_dy_em: f32,
    /// ... and has at least this many characters.
    pub attached_min_chars: usize,

    // --- text between a drawing and its caption ---
    /// Lines between the drawing and the caption may overlap either by this.
    pub between_tol_pt: f32,
    /// Such a line overlaps the drawing by more than this fraction of its
    /// width.
    pub between_overlap_ratio: f32,

    // --- final region ---
    /// The region stops this far before a caption.
    pub caption_clearance_pt: f32,
    /// Clipping against a caption never makes the region lower than this.
    pub min_region_height_pt: f32,
    /// The final region is grown by this margin before it is rendered.
    pub region_margin_pt: f32,
    /// Rules inside a region shorter than this are not table rules.
    pub inner_rule_min_pt: f32,
    /// A filled, unstroked path with at most this many elements is cell
    /// shading.
    pub shading_max_elements_count: u16,
    /// A region with more than this many other paths (not rules, not
    /// shading) is complex.
    pub complex_other_paths_count: usize,
}

impl Default for FloatsConfig {
    #[allow(clippy::too_many_lines, reason = "a flat list of default values, no logic")]
    fn default() -> Self {
        Self {
            body_min_chars: 45,
            body_min_width_em: 18.0,
            caption_label_bold_tenths: 8,
            caption_lookback_count: 40,
            continuation_min_dy_em: 0.4,
            continuation_max_dy_em: 1.45,
            continuation_size_tol_pt: 0.3,
            continuation_overlap_ratio: 0.6,
            continuation_min_chars: 20,
            caption_first_step_max_em: 1.38,
            caption_step_max_leading: 1.15,
            caption_step_slack_pt: 0.5,
            caption_step_min_em: 0.5,
            caption_size_tol_pt: 0.4,
            caption_left_overhang_em: 2.0,
            caption_right_overhang_pt: 2.0,
            caption_next_label_tol_pt: 2.0,
            caption_word_space_em: 0.3,
            caption_short_line_slack_pt: 2.0,
            caption_max_lines_count: 60,
            caption_rule_min_width_pt: 30.0,
            caption_rule_clear_em: 0.6,
            label_join_baseline_tol_pt: 0.6,
            label_join_size_tol_pt: 0.4,
            label_join_max_gap_em: 4.0,
            grid_cell_pt: 4.0,
            cluster_dilate_pt: 3.0,
            background_image_area_ratio: 0.92,
            background_path_extent_ratio: 0.95,
            min_path_alpha: 8,
            ruled_min_paths_count: 3,
            ruled_rule_tenths: 9,
            booktabs_min_rule_width_pt: 30.0,
            booktabs_max_rules_count: 600,
            booktabs_extent_tol_pt: 3.0,
            booktabs_min_height_pt: 12.0,
            booktabs_caption_overlap_ratio: 0.3,
            booktabs_max_rule_gap_pt: 420.0,
            min_component_pt: 12.0,
            page_sized_area_ratio: 0.8,
            page_sized_min_lines_count: 30,
            rule_table_containment_tol_pt: 2.0,
            vote_overlap_ratio: 0.5,
            vote_min_gap_pt: -4.0,
            vote_max_gap_pt: 30.0,
            pair_overlap_ratio: 0.5,
            side_tol_pt: 4.0,
            pair_max_dist_pt: 48.0,
            body_between_overlap_ratio: 0.5,
            penalty_figure_below_pt: 14.0,
            penalty_table_against_convention_pt: 30.0,
            penalty_table_above_pt: 3.0,
            penalty_algorithm_above_pt: 3.0,
            penalty_figure_on_table_pt: 10.0,
            penalty_table_on_drawing_pt: 6.0,
            misalign_weight_pt: 40.0,
            grow_touch_pt: 3.0,
            grow_vertical_overlap_ratio: 0.3,
            grow_max_vgap_pt: 32.0,
            grow_caption_margin_pt: 12.0,
            grow_horizontal_overlap_ratio: 0.3,
            grow_max_hgap_pt: 28.0,
            graphic_image_min_pt: 30.0,
            graphic_min_paths_count: 8,
            graphic_min_width_pt: 60.0,
            graphic_min_height_pt: 40.0,
            graphic_min_rules_count: 3,
            graphic_merge_pt: 8.0,
            inline_rule_band_ratio: 0.2,
            inline_rule_max_gap_em: 0.6,
            fallback_prose_min_chars: 50,
            fallback_prose_width_ratio: 0.8,
            fallback_prose_min_width_pt: 150.0,
            fallback_x_margin_pt: 40.0,
            fallback_column_offset_ratio: 0.15,
            fallback_edge_tol_pt: 2.0,
            fallback_max_row_gap_em: 2.4,
            fallback_min_rows_count: 3,
            fallback_min_height_em: 2.5,
            absorb_max_passes_count: 4,
            absorb_probe_pt: 5.0,
            absorb_inside_tol_pt: 1.0,
            absorb_touch_y_ratio: 0.4,
            absorb_touch_x_ratio: 0.5,
            label_min_gap_pt: -2.0,
            label_max_vgap_pt: 14.0,
            label_max_hgap_pt: 16.0,
            label_overlap_ratio: 0.8,
            side_label_max_chars: 30,
            title_center_tol_ratio: 0.1,
            title_min_width_pt: 40.0,
            title_min_indent_pt: 15.0,
            title_edge_tol_pt: 4.0,
            label_max_passes_count: 6,
            heading_size_tol_pt: 0.5,
            heading_min_chars: 4,
            heading_bold_ratio: 0.6,
            attached_size_tol_pt: 0.4,
            attached_min_dy_em: 0.5,
            attached_max_dy_em: 1.6,
            attached_min_chars: 30,
            between_tol_pt: 2.0,
            between_overlap_ratio: 0.5,
            caption_clearance_pt: 0.5,
            min_region_height_pt: 1.0,
            region_margin_pt: 3.0,
            inner_rule_min_pt: 6.0,
            shading_max_elements_count: 6,
            complex_other_paths_count: 2,
        }
    }
}

/// A figure, table, algorithm or uncaptioned graphic found on a page.
#[derive(Debug, Clone)]
pub struct Float {
    /// What the float is, from its caption label (`Graphic` without one).
    pub kind: FloatKind,
    /// Index of the page it is on.
    pub page: usize,
    /// Area of the page to render as the float's image.
    pub region: Option<Rect>,
    /// The caption lines, in reading order; empty for an uncaptioned graphic.
    pub caption: Vec<Line>,
    /// E.g. "Figure 2".
    pub label: Option<String>,
    /// Region plus caption.
    pub bbox: Rect,
    /// Text found inside the region (labels, table cells), for alt text.
    pub inner_text: String,
    /// The lines inside the region.
    pub inner_lines: Vec<Line>,
    /// The region looks like a table (ruled, or tabular rows of text).
    pub table_like: bool,
    /// Horizontal rules inside the region with their line width.
    pub hrules: Vec<(Rect, f32)>,
    /// Vertical rules inside the region.
    pub vrules: Vec<Rect>,
    /// The region contains images or drawings other than rules and fills.
    pub complex: bool,
}

/// A caption found on the page.
struct Caption {
    /// Indices of its lines in `PageData::lines`, top to bottom.
    lines: Vec<usize>,
    /// Union of the line boxes.
    bbox: Rect,
    /// Kind of float the label announces.
    kind: FloatKind,
    /// Normalised label, e.g. "Figure 2".
    label: String,
}

/// A connected group of graphics (images, paths, rotated text), or a group
/// of booktabs rules, that a caption may claim.
#[derive(Debug, Clone)]
struct Component {
    /// Union of the element boxes.
    bbox: Rect,
    /// Number of paths and rotated texts.
    n_paths: usize,
    /// Number of images.
    n_images: usize,
    /// Made (almost) only of straight rules: a ruled table.
    table_like: bool,
    /// Number of paths that are horizontal or vertical rules.
    n_rules: usize,
    /// Index of the caption that claimed it.
    owner: Option<usize>,
}

/// One graphic element of the page, as input to the clustering.
struct GraphicElement {
    /// Its bounding box.
    bbox: Rect,
    /// It is an image (otherwise a path or rotated text).
    image: bool,
    /// It is a path that is a horizontal or vertical rule.
    rule: bool,
}

/// A coarse occupancy grid over the page, used to cluster graphic elements
/// into connected components.
struct CellGrid {
    /// Number of columns.
    width: usize,
    /// Number of rows.
    height: usize,
    /// Side of a cell.
    cell_pt: f32,
    /// Growth of each element before it is drawn into the grid.
    dilate_pt: f32,
    /// Occupied cells, row by row.
    filled: Vec<bool>,
}

impl CellGrid {
    /// An empty grid covering `page`, with the cell size and dilation of
    /// `cfg`.
    fn new(page: &PageData, cfg: &FloatsConfig) -> Self {
        let width = (page.width / cfg.grid_cell_pt).ceil() as usize + GRID_PADDING_CELLS;
        let height = (page.height / cfg.grid_cell_pt).ceil() as usize + GRID_PADDING_CELLS;
        Self {
            width,
            height,
            cell_pt: cfg.grid_cell_pt,
            dilate_pt: cfg.cluster_dilate_pt,
            filled: vec![false; width * height],
        }
    }

    /// The cell index of coordinate `coord` along an axis of `len` cells,
    /// clamped to the grid.
    fn cell_index(&self, coord: f32, len: usize) -> usize {
        ((coord / self.cell_pt).floor().max(0.0) as usize).min(len - 1)
    }

    /// The inclusive cell range `(x0, y0, x1, y1)` covered by `rect` after
    /// dilation.
    fn cell_range(&self, rect: &Rect) -> (usize, usize, usize, usize) {
        let col0 = self.cell_index(rect.x0 - self.dilate_pt, self.width);
        let row0 = self.cell_index(rect.y0 - self.dilate_pt, self.height);
        let col1 = self.cell_index(rect.x1 + self.dilate_pt, self.width);
        let row1 = self.cell_index(rect.y1 + self.dilate_pt, self.height);
        (col0, row0, col1, row1)
    }

    /// The index in `filled` of the first cell covered by `rect`.
    fn first_cell(&self, rect: &Rect) -> usize {
        let (col0, row0, _, _) = self.cell_range(rect);
        row0 * self.width + col0
    }

    /// Marks the cells covered by the dilated `rect` as occupied.
    fn mark(&mut self, rect: &Rect) {
        let (col0, row0, col1, row1) = self.cell_range(rect);
        for row in row0..=row1 {
            self.filled[row * self.width + col0..=row * self.width + col1].fill(true);
        }
    }

    /// Labels the connected components of occupied cells (4-connectivity)
    /// by flood fill, in row-major order of their first cell. Returns the
    /// label of every cell ([`UNLABELLED`] for empty cells) and the number
    /// of components.
    fn label_components(&self) -> (Vec<u32>, u32) {
        let width = self.width;
        let n_cells = width * self.height;
        let mut labels = vec![UNLABELLED; n_cells];
        let mut n_labels = 0u32;
        let mut stack = Vec::new();
        for start in 0..n_cells {
            if !self.filled[start] || labels[start] != UNLABELLED {
                continue;
            }
            labels[start] = n_labels;
            stack.push(start);
            while let Some(cell) = stack.pop() {
                let (x, y) = (cell % width, cell / width);
                let mut visit = |neighbour: usize| {
                    if self.filled[neighbour] && labels[neighbour] == UNLABELLED {
                        labels[neighbour] = n_labels;
                        stack.push(neighbour);
                    }
                };
                if x > 0 {
                    visit(cell - 1);
                }
                if x + 1 < width {
                    visit(cell + 1);
                }
                if y > 0 {
                    visit(cell - width);
                }
                if y + 1 < self.height {
                    visit(cell + width);
                }
            }
            n_labels += 1;
        }
        (labels, n_labels)
    }
}

/// A float being built: its region grows while text is absorbed.
struct PendingFloat {
    /// What the float is.
    kind: FloatKind,
    /// Its region so far, if it has one.
    region: Option<Rect>,
    /// Index of its caption, `None` for an uncaptioned graphic.
    cap: Option<usize>,
    /// The region is above the caption.
    above: bool,
    /// The region looks like a table.
    table_like: bool,
}

/// Everything about the page that the steps of [`detect_floats`] read.
struct PageContext<'a> {
    /// The page.
    page: &'a PageData,
    /// Document-wide font statistics.
    stats: &'a DocStats,
    /// The document's fonts.
    fonts: &'a [FontInfo],
    /// The captions of the page.
    captions: &'a [Caption],
    /// For every line of the page, whether it belongs to a caption.
    is_caption_line: &'a [bool],
    /// The thresholds.
    cfg: &'a FloatsConfig,
}

impl PageContext<'_> {
    /// Whether a line of running prose (not a caption line) lies vertically
    /// between `top` and `bottom` and horizontally over `column`: a caption
    /// and a drawing with prose between them do not belong together.
    fn body_between(&self, top: f32, bottom: f32, column: &Rect) -> bool {
        self.page.lines.iter().enumerate().any(|(i, line)| {
            !self.is_caption_line[i]
                && line.bbox.cy() > top
                && line.bbox.cy() < bottom
                && line.bbox.x_overlap(column) > self.cfg.body_between_overlap_ratio * line.bbox.width()
                && body_like(line, self.stats, self.cfg)
        })
    }
}

/// A caption label can be separated from its text by a gap wide enough to
/// break the line in two ("Table 2:      SQuAD results ..."). Joins a bare
/// label with the text that follows it on the same baseline.
///
/// The nearest line to the right on the same baseline and of the same size
/// is merged into the label line; lines that are not bare labels, or have no
/// such neighbour, are left alone.
pub fn join_caption_labels(page: &mut PageData, cfg: &FloatsConfig, lines_cfg: &LinesConfig) {
    let mut i = 0;
    while i < page.lines.len() {
        let text = page.lines[i].text();
        let bare = CAPTION
            .find(&text)
            .is_some_and(|label| label.end() == text.trim_end().len());
        if !bare {
            i += 1;
            continue;
        }
        let (baseline, size, x1) = (page.lines[i].baseline, page.lines[i].size, page.lines[i].bbox.x1);
        let next = page
            .lines
            .iter()
            .enumerate()
            .filter(|(j, line)| {
                *j != i
                    && (line.baseline - baseline).abs() < cfg.label_join_baseline_tol_pt
                    && (line.size - size).abs() < cfg.label_join_size_tol_pt
                    && line.bbox.x0 > x1
                    && line.bbox.x0 - x1 < cfg.label_join_max_gap_em * size
            })
            .min_by(|a, b| a.1.bbox.x0.total_cmp(&b.1.bbox.x0))
            .map(|(j, _)| j);
        let Some(j) = next else {
            i += 1;
            continue;
        };
        let other = page.lines.remove(j);
        let label_idx = if j < i { i - 1 } else { i };
        let label = page.lines[label_idx].clone();
        page.lines[label_idx] = super::lines::merge_row(page.index, vec![label, other], lines_cfg);
        i = label_idx + 1;
    }
}

/// Collects evidence on whether table captions are set above or below the
/// table in this document: returns (votes for above, votes for below) from
/// captions that have a table on one side only. A page without table
/// captions returns `(0, 0)`.
pub fn table_caption_votes(
    page: &PageData,
    stats: &DocStats,
    fonts: &[FontInfo],
    cfg: &FloatsConfig,
) -> (usize, usize) {
    let captions = find_captions(page, stats.leading, fonts, cfg);
    if !captions.iter().any(|caption| caption.kind == FloatKind::Table) {
        return (0, 0);
    }
    let mut comps: Vec<Component> = cluster_graphics(page, cfg)
        .into_iter()
        .filter(|comp| large_enough(comp, cfg))
        .collect();
    comps.extend(rule_tables(page, &captions, cfg));
    let (mut above, mut below) = (0, 0);
    for caption in captions.iter().filter(|caption| caption.kind == FloatKind::Table) {
        let has_below = comps.iter().any(|comp| table_next_to(caption, comp, true, cfg));
        let has_above = comps.iter().any(|comp| table_next_to(caption, comp, false, cfg));
        match (has_above, has_below) {
            (false, true) => above += 1,
            (true, false) => below += 1,
            _ => {}
        }
    }
    (above, below)
}

/// Finds the floats of a page and removes their captions and the text
/// inside them from `page.lines`.
///
/// `table_caption_above`: the document's convention for table captions, if
/// it could be established (see [`table_caption_votes`]).
///
/// Captions claim the best-scoring nearby graphic component, grow their
/// region over adjacent components, and absorb the text inside and around
/// it. Large uncaptioned graphics become `Graphic` floats; table captions
/// without graphics take the tabular rows next to them. The floats are
/// returned captions first (in caption order), then uncaptioned graphics.
pub fn detect_floats(
    page: &mut PageData,
    stats: &DocStats,
    fonts: &[FontInfo],
    table_caption_above: Option<bool>,
    cfg: &FloatsConfig,
) -> Vec<Float> {
    let captions = find_captions(page, stats.leading, fonts, cfg);
    let is_caption_line = caption_line_mask(&captions, page.lines.len());
    let mut absorbed: Vec<Option<usize>> = vec![None; page.lines.len()]; // float index
    let floats = {
        let ctx = PageContext {
            page,
            stats,
            fonts,
            captions: &captions,
            is_caption_line: &is_caption_line,
            cfg,
        };
        let mut comps = candidate_components(page, &captions, cfg);
        let mut cap_region = pair_captions(&ctx, &mut comps, table_caption_above);
        grow_caption_regions(&ctx, &mut comps, &mut cap_region);
        let graphic_regions = uncaptioned_regions(&comps, cfg);
        let text_left = text_left_margin(&page.lines, cfg);
        let mut pending = pending_floats(&captions, &comps, &cap_region, graphic_regions);
        for float in pending.iter_mut() {
            tabular_fallback(&ctx, float);
        }
        for (float_idx, float) in pending.iter_mut().enumerate() {
            settle_region(&ctx, &mut absorbed, float_idx, float, text_left);
        }
        pending
            .iter()
            .enumerate()
            .filter_map(|(float_idx, float)| build_float(&ctx, &absorbed, float_idx, float))
            .collect()
    };
    remove_float_lines(&mut page.lines, &is_caption_line, &absorbed);
    floats
}

/// The float kind announced by the label word of a caption ("Table",
/// "Fig.", "Algorithm", ...).
fn caption_kind(word: &str) -> FloatKind {
    let lower = word.to_ascii_lowercase();
    if lower.contains("tab") {
        FloatKind::Table
    } else if lower.contains("algorithm") || lower.contains("listing") {
        FloatKind::Algorithm
    } else {
        FloatKind::Figure
    }
}

/// A line that looks like running prose (as opposed to a label inside a
/// figure or a table cell).
fn body_like(line: &Line, stats: &DocStats, cfg: &FloatsConfig) -> bool {
    line.char_count() >= cfg.body_min_chars && line.bbox.width() > cfg.body_min_width_em * stats.body_size
}

/// Finds the captions of a page. Lines are sorted by baseline; a line that
/// starts with a caption label starts a caption, and the following lines of
/// the same size and column are added to it. Every line belongs to at most
/// one caption.
fn find_captions(page: &PageData, body_leading: f32, fonts: &[FontInfo], cfg: &FloatsConfig) -> Vec<Caption> {
    let lines = &page.lines;
    let rules: Vec<Rect> = page
        .paths
        .iter()
        .filter(|path| path.is_hrule() && path.bbox.width() >= cfg.caption_rule_min_width_pt)
        .map(|path| path.bbox)
        .collect();
    let mut used = vec![false; lines.len()];
    let mut captions = Vec::new();
    for i in 0..lines.len() {
        if used[i] {
            continue;
        }
        let Some(caption) = caption_at(lines, &used, &rules, i, body_leading, fonts, cfg) else {
            continue;
        };
        for &j in &caption.lines {
            used[j] = true;
        }
        captions.push(caption);
    }
    captions
}

/// The caption starting at line `first`, or `None` when that line does not
/// start with a caption label, or the label is a mere reference (no
/// punctuation, not alone, not bold), or the line continues the paragraph
/// above it (unless the label is bold).
fn caption_at(
    lines: &[Line],
    used: &[bool],
    rules: &[Rect],
    first: usize,
    body_leading: f32,
    fonts: &[FontInfo],
    cfg: &FloatsConfig,
) -> Option<Caption> {
    let line = &lines[first];
    let text = line.text();
    let captures = CAPTION.captures(&text)?;
    let whole = captures.get(0).unwrap();
    let label_len = whole.as_str().chars().filter(|ch| !ch.is_whitespace()).count();
    let has_punct = captures.name("punct").is_some_and(|punct| !punct.as_str().is_empty());
    let label_alone = whole.end() == text.len();
    let label_bold = label_is_bold(line, label_len, fonts, cfg);

    if !(has_punct || label_alone || label_bold) {
        return None;
    }
    // "Figure 3. Shows ..." at the start of a line inside a sentence is
    // rejected here; a lone "Fig. 3" reference with trailing lowercase text
    // and no punctuation never gets here.
    if !label_bold && continues_paragraph(lines, first, cfg) {
        return None;
    }
    let (idxs, bbox) = collect_caption_lines(lines, used, rules, first, body_leading, cfg);
    Some(Caption {
        lines: idxs,
        bbox,
        kind: caption_kind(&captures["kind"]),
        label: format!("{} {}", captures["kind"].trim(), &captures["num"]),
    })
}

/// Whether the label (the first `label_len` non-space characters of the
/// line) is set in bold while the caption text after it is not.
fn label_is_bold(line: &Line, label_len: usize, fonts: &[FontInfo], cfg: &FloatsConfig) -> bool {
    let mut seen = 0;
    let (mut bold_label, mut bold_rest, mut n_label, mut n_rest) = (0, 0, 0, 0);
    for glyph in &line.glyphs {
        let bold = fonts[glyph.font as usize].bold;
        if seen < label_len {
            n_label += 1;
            bold_label += bold as usize;
        } else {
            n_rest += 1;
            bold_rest += bold as usize;
        }
        seen += glyph.text.chars().count();
    }
    n_label > 0 && bold_label * 10 >= n_label * cfg.caption_label_bold_tenths && (n_rest == 0 || bold_rest * 2 < n_rest)
}

/// Whether line `idx` continues a paragraph: a line of the same size right
/// above it, at normal leading, that overlaps it horizontally.
fn continues_paragraph(lines: &[Line], idx: usize, cfg: &FloatsConfig) -> bool {
    let line = &lines[idx];
    lines[..idx].iter().rev().take(cfg.caption_lookback_count).any(|above| {
        let dy = line.baseline - above.baseline;
        dy > cfg.continuation_min_dy_em * line.size
            && dy < cfg.continuation_max_dy_em * line.size
            && (above.size - line.size).abs() < cfg.continuation_size_tol_pt
            && above.bbox.x_overlap(&line.bbox)
                > cfg.continuation_overlap_ratio * line.bbox.width().min(above.bbox.width())
            && above.char_count() > cfg.continuation_min_chars
    })
}

/// Baseline distances a caption's lines are measured against.
#[derive(Clone, Copy)]
struct Leading {
    /// Between the caption's first two lines, once known.
    caption: Option<f32>,
    /// Of the running text.
    body: f32,
}

/// Collects the lines of the caption that starts at line `first` (`rules` are
/// the page's horizontal rules): returns
/// their indices (top to bottom) and their bounding box. The caption ends at
/// a gap, at the next caption, or after a line that ended early.
fn collect_caption_lines(
    lines: &[Line],
    used: &[bool],
    rules: &[Rect],
    first: usize,
    body_leading: f32,
    cfg: &FloatsConfig,
) -> (Vec<usize>, Rect) {
    let head_size = lines[first].size;
    let mut idxs = vec![first];
    let mut bbox = lines[first].bbox;
    let mut prev = first;
    let mut leading: Option<f32> = None;
    loop {
        let prev_line = &lines[prev];
        let Some(j) = next_caption_line(
            lines,
            used,
            prev,
            head_size,
            &bbox,
            Leading {
                caption: leading,
                body: body_leading,
            },
            cfg,
        ) else {
            break;
        };
        let next = &lines[j];
        if starts_next_caption(next, &bbox, cfg) || rule_between(rules, prev_line, next, cfg) {
            break;
        }
        // The previous line ended early although the next word would
        // have fit: the caption is over.
        if idxs.len() > 1
            && prev_line.bbox.x1 + cfg.caption_word_space_em * head_size + next.first_word_width()
                < bbox.x1 - cfg.caption_short_line_slack_pt
        {
            break;
        }
        if leading.is_none() {
            leading = Some(next.baseline - prev_line.baseline);
        }
        idxs.push(j);
        bbox = bbox.union(&next.bbox);
        prev = j;
        if idxs.len() > cfg.caption_max_lines_count {
            break;
        }
    }
    (idxs, bbox)
}

/// Whether a horizontal rule lies between the lines `upper` and `lower` and
/// spans `lower`: the table's top rule, which ends a caption set above it.
fn rule_between(rules: &[Rect], upper: &Line, lower: &Line, cfg: &FloatsConfig) -> bool {
    rules.iter().any(|rule| {
        let y = rule.cy();
        y > upper.baseline
            && y < lower.baseline - cfg.caption_rule_clear_em * lower.size
            && rule.x0 <= lower.bbox.x0
            && rule.x1 >= lower.bbox.x1
    })
}

/// The index of the line that may continue a caption after line `prev`:
/// the first unused line below it, within the expected leading (`leading`,
/// measured between the first two lines, or a default from `head_size`),
/// of the caption's size and within its horizontal extent `bbox`. `None`
/// when the next line is too far below.
fn next_caption_line(
    lines: &[Line],
    used: &[bool],
    prev: usize,
    head_size: f32,
    bbox: &Rect,
    leading: Leading,
    cfg: &FloatsConfig,
) -> Option<usize> {
    let prev_line = &lines[prev];
    for (j, line) in lines.iter().enumerate().skip(prev + 1) {
        let dy = line.baseline - prev_line.baseline;
        let max_dy = match leading.caption {
            Some(lead) => cfg.caption_step_max_leading * lead + cfg.caption_step_slack_pt,
            None => (cfg.caption_step_max_leading * leading.body + cfg.caption_step_slack_pt)
                .max(cfg.caption_first_step_max_em * head_size),
        };
        if dy > max_dy {
            return None;
        }
        if used[j] || dy < cfg.caption_step_min_em * head_size {
            continue;
        }
        // Smaller text set inside the caption (URLs in a footnote-sized
        // font) can make a line's dominant size differ: the line still
        // continues the caption when some of its glyphs have the caption's
        // size, or when it goes on with the size the previous line ended in.
        let near = |a: f32, b: f32| (a - b).abs() <= cfg.caption_size_tol_pt;
        let visible = |glyph: &&LineGlyph| !glyph.text.trim().is_empty();
        let prev_tail = prev_line.glyphs.iter().rev().find(visible).map(|glyph| glyph.size);
        if !near(line.size, head_size)
            && !line
                .glyphs
                .iter()
                .filter(visible)
                .any(|glyph| near(glyph.size, head_size))
            && !prev_tail.is_some_and(|size| near(line.size, size))
        {
            continue;
        }
        if line.bbox.x_overlap(bbox) <= 0.0 {
            continue;
        }
        if line.bbox.x0 < bbox.x0 - cfg.caption_left_overhang_em * head_size
            || line.bbox.x1 > bbox.x1 + cfg.caption_right_overhang_pt
        {
            continue;
        }
        return Some(j);
    }
    None
}

/// Whether `line`, directly below a caption with box `bbox`, starts the
/// next caption: it is aligned with the caption's left edge and starts with
/// a punctuated label of its own.
fn starts_next_caption(line: &Line, bbox: &Rect, cfg: &FloatsConfig) -> bool {
    if line.bbox.x0 <= bbox.x0 + cfg.caption_next_label_tol_pt {
        let text = line.text();
        CAPTION
            .captures(&text)
            .is_some_and(|captures| captures.name("punct").is_some_and(|punct| !punct.as_str().is_empty()))
    } else {
        false
    }
}

/// Clusters the images, paths and rotated text of the page into connected
/// components (elements closer than the dilation join). Returns an empty
/// list when the page has no graphics.
fn cluster_graphics(page: &PageData, cfg: &FloatsConfig) -> Vec<Component> {
    let elements = graphic_elements(page, cfg);
    if elements.is_empty() {
        return Vec::new();
    }
    let mut grid = CellGrid::new(page, cfg);
    for element in &elements {
        grid.mark(&element.bbox);
    }
    let (labels, n_labels) = grid.label_components();
    let mut comps: Vec<Component> = (0..n_labels)
        .map(|_| Component {
            bbox: Rect::EMPTY,
            n_paths: 0,
            n_images: 0,
            table_like: false,
            n_rules: 0,
            owner: None,
        })
        .collect();
    for element in &elements {
        let comp = &mut comps[labels[grid.first_cell(&element.bbox)] as usize];
        comp.bbox = comp.bbox.union(&element.bbox);
        if element.image {
            comp.n_images += 1;
        } else {
            comp.n_paths += 1;
            comp.n_rules += element.rule as usize;
        }
    }
    // A drawing made (almost) only of straight rules is a ruled table.
    for comp in comps.iter_mut() {
        comp.table_like = comp.n_images == 0
            && comp.n_paths >= cfg.ruled_min_paths_count
            && comp.n_rules * 10 >= comp.n_paths * cfg.ruled_rule_tenths;
    }
    comps
}

/// The graphic elements of the page that can be part of a float: images
/// that are not page backgrounds, rotated text (axis labels belong to the
/// drawing they are part of, arXiv stamps do not), and visible paths that
/// are not page frames or white fills.
fn graphic_elements(page: &PageData, cfg: &FloatsConfig) -> Vec<GraphicElement> {
    let page_area = page.width * page.height;
    let mut elements: Vec<GraphicElement> = Vec::new();
    for image in &page.images {
        if image.bbox.area() > cfg.background_image_area_ratio * page_area {
            continue;
        }
        elements.push(GraphicElement {
            bbox: image.bbox,
            image: true,
            rule: false,
        });
    }
    for rotated in &page.rotated {
        let compact: String = rotated.text.chars().filter(|ch| !ch.is_whitespace()).collect();
        if compact.starts_with("arXiv:") || rotated.bbox.is_unset() {
            continue;
        }
        elements.push(GraphicElement {
            bbox: rotated.bbox,
            image: false,
            rule: false,
        });
    }
    for path in &page.paths {
        if path.bbox.width() > cfg.background_path_extent_ratio * page.width
            && path.bbox.height() > cfg.background_path_extent_ratio * page.height
        {
            continue;
        }
        if path.filled && !path.stroked && path.is_white() {
            continue;
        }
        if path.color[3] < cfg.min_path_alpha {
            continue;
        }
        if path.is_hrule() && inline_rule(page, &path.bbox, cfg) {
            continue;
        }
        elements.push(GraphicElement {
            bbox: path.bbox,
            image: false,
            rule: path.is_hrule() || path.is_vrule(),
        });
    }
    elements
}

/// Whether a horizontal rule is a fraction bar in an equation
/// ("R(T) ≤ D²/2α …"): its middle is inside a line of text that goes on to
/// its left or right, and a numerator and a denominator sit right above and
/// below it, within its width. Table rules sit between rows with nothing
/// running through them; chart gridlines have labels beside them but no
/// text stacked on them.
fn inline_rule(page: &PageData, rule: &Rect, cfg: &FloatsConfig) -> bool {
    let cy = rule.cy();
    // Lines whose middle band holds the bar and that reach it: the text runs
    // across the bar (a numerator glyph's line), or a piece of the formula
    // sits right next to it ("+", the next fraction). The other column's
    // text beside a table rule is a gutter away.
    let reaching = |side: Option<bool>| {
        page.lines.iter().any(|line| {
            let b = &line.bbox;
            let band = cfg.inline_rule_band_ratio * b.height();
            let beside = b.x0 < rule.x0 - line.size || b.x1 > rule.x1 + line.size;
            let close = cfg.inline_rule_max_gap_em * line.size;
            let placed = match side {
                None => b.x0 < rule.x1 && b.x1 > rule.x0,
                Some(true) => b.x1 <= rule.x0 + line.size && rule.x0 - b.x1 <= close,
                Some(false) => b.x0 >= rule.x1 - line.size && b.x0 - rule.x1 <= close,
            };
            cy > b.y0 + band && cy < b.y1 - band && beside && placed
        })
    };
    let through = reaching(None) || reaching(Some(true)) || reaching(Some(false));
    // Glyphs, not lines: numerators of several fractions on one baseline
    // make one wide line.
    let stacked = |above: bool| {
        page.lines.iter().any(|line| {
            line.glyphs.iter().any(|glyph| {
                let ink = &glyph.ink;
                let gap = if above { cy - ink.y1 } else { ink.y0 - cy };
                !ink.is_unset() && ink.cx() > rule.x0 && ink.cx() < rule.x1 && gap > 0.0 && gap < line.size
            })
        })
    };
    through && stacked(true) && stacked(false)
}

/// Groups of horizontal rules with the same extent: tables set with booktabs
/// (or algorithm floats) have no other graphics. Returns one table-like
/// component per group of at least two rules; a group is split where a
/// caption sits between two rules or the rules are implausibly far apart.
fn rule_tables(page: &PageData, captions: &[Caption], cfg: &FloatsConfig) -> Vec<Component> {
    let mut rules: Vec<Rect> = page
        .paths
        .iter()
        .filter(|path| {
            path.is_hrule()
                && path.bbox.width() >= cfg.booktabs_min_rule_width_pt
                && !(path.filled && !path.stroked && path.is_white())
                && !inline_rule(page, &path.bbox, cfg)
        })
        .map(|path| path.bbox)
        .collect();
    if rules.len() < 2 || rules.len() > cfg.booktabs_max_rules_count {
        return Vec::new();
    }
    rules.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let mut out = Vec::new();
    for (_, group) in group_rules_by_extent(&rules, cfg) {
        if group.len() < 2 {
            continue;
        }
        split_rule_group(&group, captions, &mut out, cfg);
    }
    out
}

/// Groups `rules` (sorted top to bottom) into sets that start and end at
/// the same x, transitively (union-find). Each group keeps the rules in
/// their order; groups are keyed by their root index.
fn group_rules_by_extent(rules: &[Rect], cfg: &FloatsConfig) -> BTreeMap<usize, Vec<Rect>> {
    let mut group_of: Vec<usize> = (0..rules.len()).collect();
    for i in 0..rules.len() {
        for j in i + 1..rules.len() {
            if (rules[i].x0 - rules[j].x0).abs() <= cfg.booktabs_extent_tol_pt
                && (rules[i].x1 - rules[j].x1).abs() <= cfg.booktabs_extent_tol_pt
            {
                let (a, b) = (find_root(&mut group_of, i), find_root(&mut group_of, j));
                if a != b {
                    group_of[b] = a;
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<Rect>> = BTreeMap::default();
    for (i, rule) in rules.iter().enumerate() {
        let root = find_root(&mut group_of, i);
        groups.entry(root).or_default().push(*rule);
    }
    groups
}

/// The root of `i` in the union-find forest `parents`, compressing the path
/// on the way.
fn find_root(parents: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while parents[root] != root {
        root = parents[root];
    }
    let mut cur = i;
    while parents[cur] != root {
        let next = parents[cur];
        parents[cur] = root;
        cur = next;
    }
    root
}

/// Splits a group of aligned rules (top to bottom) into tables where a
/// caption starts between two rules, or where two rules are implausibly far
/// apart, and appends each table to `out`.
fn split_rule_group(group: &[Rect], captions: &[Caption], out: &mut Vec<Component>, cfg: &FloatsConfig) {
    let mut cur: Vec<Rect> = vec![group[0]];
    for pair in group.windows(2) {
        let (upper, lower) = (&pair[0], &pair[1]);
        let cap_between = captions.iter().any(|caption| {
            caption.bbox.cy() > upper.y1
                && caption.bbox.cy() < lower.y0
                && caption.bbox.x_overlap(upper)
                    > cfg.booktabs_caption_overlap_ratio * caption.bbox.width().min(upper.width())
        });
        if cap_between || lower.y0 - upper.y1 > cfg.booktabs_max_rule_gap_pt {
            flush_rule_table(&mut cur, out, cfg);
        }
        cur.push(*lower);
    }
    flush_rule_table(&mut cur, out, cfg);
}

/// Turns the rules collected in `cur` into a table component (if there are
/// at least two and they are high enough), appends it to `out`, and clears
/// `cur`.
fn flush_rule_table(cur: &mut Vec<Rect>, out: &mut Vec<Component>, cfg: &FloatsConfig) {
    if cur.len() >= 2 {
        let bbox = cur.iter().fold(Rect::EMPTY, |acc, rule| acc.union(rule));
        if bbox.height() >= cfg.booktabs_min_height_pt {
            out.push(Component {
                bbox,
                n_paths: cur.len(),
                n_images: 0,
                table_like: true,
                n_rules: cur.len(),
                owner: None,
            });
        }
    }
    cur.clear();
}

/// Whether a component is large enough in both directions to be a float.
fn large_enough(comp: &Component, cfg: &FloatsConfig) -> bool {
    comp.bbox.width() >= cfg.min_component_pt && comp.bbox.height() >= cfg.min_component_pt
}

/// Whether `comp` sits right below (`table_below`) or right above the
/// caption, overlapping it horizontally: evidence for the caption position
/// convention.
fn table_next_to(caption: &Caption, comp: &Component, table_below: bool, cfg: &FloatsConfig) -> bool {
    let overlaps =
        caption.bbox.x_overlap(&comp.bbox) >= cfg.vote_overlap_ratio * caption.bbox.width().min(comp.bbox.width());
    let gap = if table_below {
        comp.bbox.y0 - caption.bbox.y1
    } else {
        caption.bbox.y0 - comp.bbox.y1
    };
    overlaps && (cfg.vote_min_gap_pt..=cfg.vote_max_gap_pt).contains(&gap)
}

/// For every line of the page, whether it belongs to one of `captions`.
fn caption_line_mask(captions: &[Caption], n_lines: usize) -> Vec<bool> {
    let mut mask = vec![false; n_lines];
    for caption in captions {
        for &i in &caption.lines {
            mask[i] = true;
        }
    }
    mask
}

/// The graphic components a caption may claim: clustered graphics that are
/// large enough and not page-sized decorations, plus booktabs tables that
/// are not part of a drawing (a table that contains drawings replaces them).
fn candidate_components(page: &PageData, captions: &[Caption], cfg: &FloatsConfig) -> Vec<Component> {
    let page_area = page.width * page.height;
    let mut comps: Vec<Component> = cluster_graphics(page, cfg)
        .into_iter()
        .filter(|comp| large_enough(comp, cfg))
        .filter(|comp| {
            !(comp.bbox.area() > cfg.page_sized_area_ratio * page_area
                && page.lines.len() > cfg.page_sized_min_lines_count)
        })
        .collect();
    let tol = cfg.rule_table_containment_tol_pt;
    let tables = rule_tables(page, captions, cfg);
    split_side_by_side_tables(&mut comps, &tables, captions, tol);
    for table in tables {
        if comps
            .iter()
            .any(|comp| comp.bbox.inflate(tol, tol).contains(&table.bbox))
        {
            continue;
        }
        comps.retain(|comp| !table.bbox.inflate(tol, tol).contains(&comp.bbox));
        comps.push(table);
    }
    comps
}

/// Replaces a ruled drawing that is really two tables set side by side (their
/// borders are closer than the clustering distance) by those tables.
///
/// `tables` are the tables found from the rules alone. A table-like
/// component is split when it contains at least two of them, none overlapping
/// another horizontally, and each has a table caption over its own columns
/// only. `tol` is the slack (in points) of the containment test.
fn split_side_by_side_tables(comps: &mut Vec<Component>, tables: &[Component], captions: &[Caption], tol: f32) {
    let own_caption = |table: &Component, others: &[&Component]| {
        captions.iter().any(|caption| {
            caption.kind == FloatKind::Table
                && caption.bbox.x_overlap(&table.bbox) > 0.0
                && others.iter().all(|other| caption.bbox.x_overlap(&other.bbox) <= 0.0)
        })
    };
    let mut split: Vec<Component> = Vec::new();
    comps.retain(|comp| {
        let inside: Vec<&Component> = tables
            .iter()
            .filter(|table| comp.bbox.inflate(tol, tol).contains(&table.bbox))
            .collect();
        let disjoint = inside.iter().enumerate().all(|(i, table)| {
            inside
                .iter()
                .skip(i + 1)
                .all(|other| table.bbox.x_overlap(&other.bbox) <= 0.0)
        });
        let captioned = inside.iter().enumerate().all(|(i, table)| {
            let others: Vec<&Component> = inside
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, other)| *other)
                .collect();
            own_caption(table, &others)
        });
        let side_by_side = comp.table_like && inside.len() >= 2 && disjoint && captioned;
        if side_by_side {
            split.extend(inside.into_iter().cloned());
        }
        !side_by_side
    });
    comps.extend(split);
}

/// Assigns each caption at most one component, best score first, each
/// component to at most one caption. Marks the claimed components' owner and
/// returns, per caption, the claimed region and whether it is above the
/// caption.
fn pair_captions(
    ctx: &PageContext,
    comps: &mut [Component],
    table_caption_above: Option<bool>,
) -> Vec<Option<(Rect, bool)>> {
    // Seed assignment: (score, caption, component, above?).
    let mut pairs: Vec<(f32, usize, usize, bool)> = Vec::new();
    for (cap_idx, caption) in ctx.captions.iter().enumerate() {
        for (comp_idx, comp) in comps.iter().enumerate() {
            if let Some((score, above)) = pair_score(ctx, caption, comp, table_caption_above) {
                pairs.push((score, cap_idx, comp_idx, above));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cap_region: Vec<Option<(Rect, bool)>> = vec![None; ctx.captions.len()];
    for (_, cap_idx, comp_idx, above) in pairs {
        if cap_region[cap_idx].is_some() || comps[comp_idx].owner.is_some() {
            continue;
        }
        comps[comp_idx].owner = Some(cap_idx);
        cap_region[cap_idx] = Some((comps[comp_idx].bbox, above));
    }
    cap_region
}

/// The score (lower is better) of `caption` claiming `comp`, and whether the
/// component is above the caption. `None` when they cannot belong together:
/// no horizontal overlap, the component is beside the caption or too far,
/// or prose lies between them.
fn pair_score(
    ctx: &PageContext,
    caption: &Caption,
    comp: &Component,
    table_caption_above: Option<bool>,
) -> Option<(f32, bool)> {
    let cfg = ctx.cfg;
    let (cap_box, comp_box) = (&caption.bbox, &comp.bbox);
    let overlap = cap_box.x_overlap(comp_box);
    if overlap < cfg.pair_overlap_ratio * cap_box.width().min(comp_box.width()) {
        return None;
    }
    let (dist, above) = if comp_box.y1 <= cap_box.y0 + cfg.side_tol_pt {
        ((cap_box.y0 - comp_box.y1).max(0.0), true)
    } else if comp_box.y0 >= cap_box.y1 - cfg.side_tol_pt {
        ((comp_box.y0 - cap_box.y1).max(0.0), false)
    } else {
        return None;
    };
    if dist > cfg.pair_max_dist_pt {
        return None;
    }
    let (top, bottom) = if above {
        (comp_box.y1, cap_box.y0)
    } else {
        (cap_box.y1, comp_box.y0)
    };
    if ctx.body_between(top, bottom, cap_box) {
        return None;
    }
    let penalty = side_penalty(caption.kind, above, table_caption_above, cfg);
    let mismatch = kind_mismatch(caption.kind, comp.table_like, cfg);
    let misalign = misalignment(cap_box, comp_box, cfg);
    Some((dist + penalty + mismatch + misalign, above))
}

/// Score penalty for a float on an unusual side of its caption.
/// `drawing_above`: the drawing is above its caption.
fn side_penalty(kind: FloatKind, drawing_above: bool, table_caption_above: Option<bool>, cfg: &FloatsConfig) -> f32 {
    match (kind, drawing_above) {
        (FloatKind::Figure, false) => cfg.penalty_figure_below_pt,
        (FloatKind::Table, _) => match table_caption_above {
            Some(caption_above) if caption_above == drawing_above => cfg.penalty_table_against_convention_pt,
            Some(_) => 0.0,
            None if drawing_above => cfg.penalty_table_above_pt,
            None => 0.0,
        },
        (FloatKind::Algorithm, true) => cfg.penalty_algorithm_above_pt,
        _ => 0.0,
    }
}

/// Score penalty for a caption kind that does not fit the component: a
/// ruled table fits a table caption, a drawing fits a figure.
fn kind_mismatch(kind: FloatKind, table_like: bool, cfg: &FloatsConfig) -> f32 {
    match (kind, table_like) {
        (FloatKind::Figure, true) => cfg.penalty_figure_on_table_pt,
        (FloatKind::Table, false) => cfg.penalty_table_on_drawing_pt,
        _ => 0.0,
    }
}

/// Score penalty for a caption off-centre from the component: captions are
/// centred on (or aligned with) their float.
fn misalignment(cap_box: &Rect, comp_box: &Rect, cfg: &FloatsConfig) -> f32 {
    (comp_box.cx() - cap_box.cx()).abs() / cap_box.width().max(comp_box.width()).max(1.0) * cfg.misalign_weight_pt
}

/// Grows the region of every caption over adjacent unclaimed components on
/// the same side of the caption, until nothing more joins.
fn grow_caption_regions(ctx: &PageContext, comps: &mut [Component], cap_region: &mut [Option<(Rect, bool)>]) {
    for (cap_idx, caption) in ctx.captions.iter().enumerate() {
        let Some((mut region, above)) = cap_region[cap_idx] else {
            continue;
        };
        loop {
            let mut grew = false;
            for comp in comps.iter_mut() {
                if comp.owner.is_some() {
                    continue;
                }
                if joins_region(ctx, caption, &region, above, &comp.bbox) {
                    comp.owner = Some(cap_idx);
                    region = region.union(&comp.bbox);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        cap_region[cap_idx] = Some((region, above));
    }
}

/// Whether the component with box `comp_box` joins a caption's `region`:
/// it is on the same side of the caption (`above`) and touches the region,
/// is stacked on it with no prose in between, or sits beside it.
fn joins_region(ctx: &PageContext, caption: &Caption, region: &Rect, above: bool, comp_box: &Rect) -> bool {
    let cfg = ctx.cfg;
    let same_side = if above {
        comp_box.y1 <= caption.bbox.y0 + cfg.side_tol_pt
    } else {
        comp_box.y0 >= caption.bbox.y1 - cfg.side_tol_pt
    };
    if !same_side {
        return false;
    }
    let x_ov = comp_box.x_overlap(region);
    let y_ov = comp_box.y_overlap(region);
    let touching = comp_box.intersects(&region.inflate(cfg.grow_touch_pt, cfg.grow_touch_pt));
    let vertical = x_ov > cfg.grow_vertical_overlap_ratio * comp_box.width().min(region.width())
        && -y_ov <= cfg.grow_max_vgap_pt
        && {
            let (top, bottom) = if comp_box.cy() < region.cy() {
                (comp_box.y1, region.y0)
            } else {
                (region.y1, comp_box.y0)
            };
            !ctx.body_between(top, bottom, comp_box)
        };
    let within_caption = comp_box.x0 >= caption.bbox.x0 - cfg.grow_caption_margin_pt
        && comp_box.x1 <= caption.bbox.x1 + cfg.grow_caption_margin_pt;
    let horizontal = y_ov > cfg.grow_horizontal_overlap_ratio * comp_box.height().min(region.height())
        && (within_caption || (-x_ov <= cfg.grow_max_hgap_pt && caption.lines.len() == 1));
    touching || vertical || horizontal
}

/// Regions of the unclaimed components that are large enough to matter,
/// overlapping ones merged, with whether they are (all) table-like.
fn uncaptioned_regions(comps: &[Component], cfg: &FloatsConfig) -> Vec<(Rect, bool)> {
    let mut graphic_regions: Vec<(Rect, bool)> = Vec::new();
    for comp in comps.iter().filter(|comp| comp.owner.is_none()) {
        if !significant_graphic(comp, cfg) {
            continue;
        }
        // Merge with an overlapping graphic region if there is one.
        if let Some(region) = graphic_regions.iter_mut().find(|region| {
            region
                .0
                .inflate(cfg.graphic_merge_pt, cfg.graphic_merge_pt)
                .intersects(&comp.bbox)
        }) {
            region.0 = region.0.union(&comp.bbox);
            region.1 &= comp.table_like;
        } else {
            graphic_regions.push((comp.bbox, comp.table_like));
        }
    }
    graphic_regions
}

/// Whether an uncaptioned component is worth keeping as a graphic: a large
/// enough image, a large drawing of many paths, or a ruled table.
fn significant_graphic(comp: &Component, cfg: &FloatsConfig) -> bool {
    let (width, height) = (comp.bbox.width(), comp.bbox.height());
    (comp.n_images >= 1 && width >= cfg.graphic_image_min_pt && height >= cfg.graphic_image_min_pt)
        || (!comp.table_like
            && comp.n_paths >= cfg.graphic_min_paths_count
            && width >= cfg.graphic_min_width_pt
            && height >= cfg.graphic_min_height_pt)
        || (comp.table_like && comp.n_rules >= cfg.graphic_min_rules_count)
}

/// Left margin of the running text on the page (0 when it has no long
/// line).
fn text_left_margin(lines: &[Line], cfg: &FloatsConfig) -> f32 {
    let text_left = lines
        .iter()
        .filter(|line| line.char_count() >= cfg.body_min_chars)
        .map(|line| line.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    if text_left.is_finite() { text_left } else { 0.0 }
}

/// The floats to build: one per caption (with the region it claimed, if
/// any), then one per uncaptioned graphic region.
fn pending_floats(
    captions: &[Caption],
    comps: &[Component],
    cap_region: &[Option<(Rect, bool)>],
    graphic_regions: Vec<(Rect, bool)>,
) -> Vec<PendingFloat> {
    let mut pending: Vec<PendingFloat> = Vec::new();
    for (cap_idx, caption) in captions.iter().enumerate() {
        let table_like = comps.iter().any(|comp| comp.owner == Some(cap_idx) && comp.table_like);
        pending.push(PendingFloat {
            kind: caption.kind,
            region: cap_region[cap_idx].map(|region| region.0),
            cap: Some(cap_idx),
            above: cap_region[cap_idx].map(|region| region.1).unwrap_or(true),
            table_like,
        });
    }
    for (region, table_like) in graphic_regions {
        pending.push(PendingFloat {
            kind: FloatKind::Graphic,
            region: Some(region),
            cap: None,
            above: true,
            table_like,
        });
    }
    pending
}

/// Fallback for tables without any graphics: gives a table caption that
/// claimed nothing the tabular rows below it, or else above it. Other
/// floats are left alone.
fn tabular_fallback(ctx: &PageContext, float: &mut PendingFloat) {
    if float.region.is_some() || !matches!(float.kind, FloatKind::Table) {
        return;
    }
    let caption = &ctx.captions[float.cap.expect("table floats come from a caption")];
    for &down in &[true, false] {
        if let Some(region) = tabular_rows(ctx, caption, down) {
            float.region = Some(region);
            float.above = !down;
            float.table_like = true;
            break;
        }
    }
}

/// The region of the rows of text directly below (`down`) or above the
/// caption, up to a large gap or a line of prose. `None` when there are too
/// few rows, or they are too low to be a table.
fn tabular_rows(ctx: &PageContext, caption: &Caption, down: bool) -> Option<Rect> {
    let cfg = ctx.cfg;
    let cap_box = caption.bbox;
    let prose = |line: &Line| {
        line.char_count() >= cfg.fallback_prose_min_chars
            && line.bbox.width()
                >= cfg.fallback_prose_width_ratio * cap_box.width().max(cfg.fallback_prose_min_width_pt)
    };
    let column = cap_box.inflate(cfg.fallback_x_margin_pt, 0.0);
    // A caption in one half of the page heads a table in its column: the
    // other column's lines on the same baselines are not rows of it.
    let in_column = (cap_box.cx() - ctx.page.width / 2.0).abs() > cfg.fallback_column_offset_ratio * ctx.page.width;
    let mut candidates: Vec<&Line> = ctx
        .page
        .lines
        .iter()
        .enumerate()
        .filter(|(i, line)| {
            !ctx.is_caption_line[*i]
                && line.bbox.x_overlap(&column) > 0.0
                && (!in_column || (line.bbox.x0 >= column.x0 && line.bbox.x1 <= column.x1))
                && if down {
                    line.bbox.y0 >= cap_box.y1 - cfg.fallback_edge_tol_pt
                } else {
                    line.bbox.y1 <= cap_box.y0 + cfg.fallback_edge_tol_pt
                }
        })
        .map(|(_, line)| line)
        .collect();
    candidates.sort_by(|a, b| {
        if down {
            a.bbox.y0.total_cmp(&b.bbox.y0)
        } else {
            b.bbox.y1.total_cmp(&a.bbox.y1)
        }
    });
    let mut region = Rect::EMPTY;
    let mut edge = if down { cap_box.y1 } else { cap_box.y0 };
    let mut rows = 0;
    for line in candidates {
        let gap = if down { line.bbox.y0 - edge } else { edge - line.bbox.y1 };
        if gap > cfg.fallback_max_row_gap_em * ctx.stats.body_size || prose(line) {
            break;
        }
        region = region.union(&line.bbox);
        edge = if down {
            edge.max(line.bbox.y1)
        } else {
            edge.min(line.bbox.y0)
        };
        rows += 1;
    }
    (rows >= cfg.fallback_min_rows_count && region.height() > cfg.fallback_min_height_em * ctx.stats.body_size)
        .then_some(region)
}

/// Fixes the final region of float `float_idx`: absorbs the text inside and
/// around it into `absorbed`, keeps it off every caption, and adds the
/// margin. Floats without a region are left alone.
fn settle_region(
    ctx: &PageContext,
    absorbed: &mut [Option<usize>],
    float_idx: usize,
    float: &mut PendingFloat,
    text_left: f32,
) {
    let Some(region) = float.region else { return };
    let mut region = absorb_inner_lines(ctx, absorbed, float_idx, region, float.table_like);
    // A chart drawn only with straight lines (bars, grid) looks like a ruled
    // table, but a figure caption says it is a drawing with labels.
    if !float.table_like || float.kind == FloatKind::Figure {
        region = absorb_side_labels(ctx, absorbed, float_idx, region, text_left);
    }
    if let Some(cap_idx) = float.cap {
        let caption = &ctx.captions[cap_idx];
        region = absorb_between_caption(ctx, absorbed, float_idx, region, caption, float.above);
        // Never let the image overlap its own caption.
        if float.above {
            end_above(&mut region, &caption.bbox, ctx.cfg);
        } else {
            start_below(&mut region, &caption.bbox, ctx.cfg);
        }
    }
    float.region = Some(clear_other_captions(ctx, float.cap, region));
}

/// Absorbs the lines inside or touching `region` (in up to a few passes, as
/// each absorbed line may grow the region) and returns the grown region.
/// Prose that merely touches a non-table region is not part of it.
fn absorb_inner_lines(
    ctx: &PageContext,
    absorbed: &mut [Option<usize>],
    float_idx: usize,
    mut region: Rect,
    table_like: bool,
) -> Rect {
    let cfg = ctx.cfg;
    for _ in 0..cfg.absorb_max_passes_count {
        let mut grew = false;
        let probe = region.inflate(cfg.absorb_probe_pt, cfg.absorb_probe_pt);
        for (i, line) in ctx.page.lines.iter().enumerate() {
            if ctx.is_caption_line[i] || absorbed[i].is_some() {
                continue;
            }
            if !belongs_to_region(ctx, &region, &probe, line, table_like) {
                continue;
            }
            absorbed[i] = Some(float_idx);
            let union = region.union(&line.ink.union(&line.bbox));
            if union != region {
                region = union;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    region
}

/// Whether `line` belongs to `region`: it is inside it, or it touches the
/// `probe` (the region grown a little) and is not prose (unless the region
/// is a table). Prose with its centre outside the region never belongs.
fn belongs_to_region(ctx: &PageContext, region: &Rect, probe: &Rect, line: &Line, table_like: bool) -> bool {
    let cfg = ctx.cfg;
    let tol = cfg.absorb_inside_tol_pt;
    let inside = region.inflate(tol, tol).contains(&line.bbox);
    let touches = (probe.y_overlap(&line.bbox) >= cfg.absorb_touch_y_ratio * line.bbox.height()
        && probe.x_overlap(&line.bbox) >= cfg.absorb_touch_x_ratio * line.bbox.width())
        || probe.contains_point(line.bbox.cx(), line.bbox.cy());
    if !(inside || (touches && (!body_like(line, ctx.stats, cfg) || table_like))) {
        return false;
    }
    // Prose that merely touches the region is not part of it.
    !(!inside
        && body_like(line, ctx.stats, cfg)
        && !region.inflate(tol, tol).contains_point(line.bbox.cx(), line.bbox.cy()))
}

/// Absorbs short isolated labels right above, below or beside the drawing
/// (titles of sub-figures, axis titles), as long as they are not headings
/// or paragraph ends. A few passes, each against the region grown by the
/// labels found before: a chart title sits above its axis title. Returns the
/// grown region.
fn absorb_side_labels(
    ctx: &PageContext,
    absorbed: &mut [Option<usize>],
    float_idx: usize,
    region: Rect,
    text_left: f32,
) -> Rect {
    let cfg = ctx.cfg;
    let mut grown = region;
    for _ in 0..cfg.label_max_passes_count {
        let before = grown;
        for (i, line) in ctx.page.lines.iter().enumerate() {
            if ctx.is_caption_line[i] || absorbed[i].is_some() || line.char_count() >= cfg.body_min_chars {
                continue;
            }
            // A label the grown drawing now encloses (a tick label left of
            // the axis, once the legend above widened the drawing).
            let inside = before
                .inflate(cfg.absorb_inside_tol_pt, cfg.absorb_inside_tol_pt)
                .contains(&line.bbox);
            let stacked = stacked_label(&before, line, cfg);
            if !(inside || stacked || beside_label(&before, line, cfg)) {
                continue;
            }
            if heading_like(ctx, &before, line, stacked, text_left) {
                continue;
            }
            if !attached_to_paragraph(ctx, absorbed, i) {
                absorbed[i] = Some(float_idx);
                grown = grown.union(&line.bbox);
            }
        }
        if grown == before {
            break;
        }
    }
    grown
}

/// Whether `line` sits right above or below `region`, mostly within its
/// width.
fn stacked_label(region: &Rect, line: &Line, cfg: &FloatsConfig) -> bool {
    let vgap = if line.bbox.cy() < region.cy() {
        region.y0 - line.bbox.y1
    } else {
        line.bbox.y0 - region.y1
    };
    (cfg.label_min_gap_pt..=cfg.label_max_vgap_pt).contains(&vgap)
        && line.bbox.x_overlap(region) >= cfg.label_overlap_ratio * line.bbox.width()
}

/// Whether `line` is a short label right left or right of `region`, mostly
/// within its height.
fn beside_label(region: &Rect, line: &Line, cfg: &FloatsConfig) -> bool {
    let hgap = if line.bbox.cx() < region.cx() {
        region.x0 - line.bbox.x1
    } else {
        line.bbox.x0 - region.x1
    };
    (cfg.label_min_gap_pt..=cfg.label_max_hgap_pt).contains(&hgap)
        && line.bbox.y_overlap(region) >= cfg.label_overlap_ratio * line.bbox.height()
        && line.char_count() < cfg.side_label_max_chars
}

/// Whether a label next to `region` looks like a section heading: at body
/// size and numbered, or bold -- unless it is centred over the drawing
/// (`stacked`) and away from the text margin `text_left`, which makes it the
/// drawing's title.
fn heading_like(ctx: &PageContext, region: &Rect, line: &Line, stacked: bool, text_left: f32) -> bool {
    let cfg = ctx.cfg;
    // A title over a drawing is centred on it, starts at its left edge (a
    // chart title), or is set in the drawing's own font (sans-serif labels
    // in a serif paper); a heading starts at the text margin.
    let centred =
        (line.bbox.cx() - region.cx()).abs() <= cfg.title_center_tol_ratio * region.width().max(cfg.title_min_width_pt);
    let at_edge = (line.bbox.x0 - region.x0).abs() <= cfg.title_edge_tol_pt;
    let body_sans = ctx
        .fonts
        .get(ctx.stats.body_font as usize)
        .is_some_and(|font| font.sans);
    let own_font = !body_sans && line.font_frac(ctx.fonts, |font| font.sans) >= cfg.heading_bold_ratio;
    let centred_title =
        (centred || at_edge || own_font) && line.bbox.x0 > text_left + cfg.title_min_indent_pt && stacked;
    line.size >= ctx.stats.body_size - cfg.heading_size_tol_pt
        && (HEADING_LIKE.is_match(&line.text())
            || (line.char_count() >= cfg.heading_min_chars
                && line.font_frac(ctx.fonts, |font| font.bold) >= cfg.heading_bold_ratio
                && !centred_title))
}

/// Whether line `idx` is part of a paragraph: another unabsorbed line of the
/// same size at normal leading on the far side.
fn attached_to_paragraph(ctx: &PageContext, absorbed: &[Option<usize>], idx: usize) -> bool {
    let cfg = ctx.cfg;
    let line = &ctx.page.lines[idx];
    ctx.page.lines.iter().enumerate().any(|(j, other)| {
        j != idx
            && absorbed[j].is_none()
            && (other.size - line.size).abs() < cfg.attached_size_tol_pt
            && (other.baseline - line.baseline).abs() > cfg.attached_min_dy_em * line.size
            && (other.baseline - line.baseline).abs() < cfg.attached_max_dy_em * line.size
            && other.bbox.x_overlap(&line.bbox) > 0.0
            && other.char_count() >= cfg.attached_min_chars
            // The rest of a paragraph is running text; the other entries
            // of a legend are short labels themselves.
            && body_like(other, ctx.stats, cfg)
    })
}

/// Absorbs the sub-captions and labels between the drawing `region` and its
/// `caption` (`above`: the drawing is above the caption) and returns the
/// grown region.
fn absorb_between_caption(
    ctx: &PageContext,
    absorbed: &mut [Option<usize>],
    float_idx: usize,
    mut region: Rect,
    caption: &Caption,
    above: bool,
) -> Rect {
    let cfg = ctx.cfg;
    let (top, bottom) = if above {
        (region.y1 - cfg.between_tol_pt, caption.bbox.y0 + cfg.between_tol_pt)
    } else {
        (caption.bbox.y1 - cfg.between_tol_pt, region.y0 + cfg.between_tol_pt)
    };
    for (i, line) in ctx.page.lines.iter().enumerate() {
        if ctx.is_caption_line[i] || absorbed[i].is_some() || body_like(line, ctx.stats, cfg) {
            continue;
        }
        if line.bbox.y0 >= top
            && line.bbox.y1 <= bottom
            && line.bbox.x_overlap(&region) > cfg.between_overlap_ratio * line.bbox.width()
        {
            absorbed[i] = Some(float_idx);
            region = region.union(&line.bbox);
        }
    }
    region
}

/// Moves the bottom of `region` above `obstacle`, keeping a minimum height.
fn end_above(region: &mut Rect, obstacle: &Rect, cfg: &FloatsConfig) {
    region.y1 = region
        .y1
        .min(obstacle.y0 - cfg.caption_clearance_pt)
        .max(region.y0 + cfg.min_region_height_pt);
}

/// Moves the top of `region` below `obstacle`, keeping a minimum height.
fn start_below(region: &mut Rect, obstacle: &Rect, cfg: &FloatsConfig) {
    region.y0 = region
        .y0
        .max(obstacle.y1 + cfg.caption_clearance_pt)
        .min(region.y1 - cfg.min_region_height_pt);
}

/// Adds the margin to `region`, clips it to the page and keeps it off every
/// caption other than its own (`own_caption`).
fn clear_other_captions(ctx: &PageContext, own_caption: Option<usize>, region: Rect) -> Rect {
    let margin = ctx.cfg.region_margin_pt;
    let mut region = region.inflate(margin, margin).intersect(&ctx.page.rect());
    for (cap_idx, caption) in ctx.captions.iter().enumerate() {
        if own_caption == Some(cap_idx) || !region.intersects(&caption.bbox) {
            continue;
        }
        if caption.bbox.cy() < region.cy() {
            start_below(&mut region, &caption.bbox, ctx.cfg);
        } else {
            end_above(&mut region, &caption.bbox, ctx.cfg);
        }
    }
    region
}

/// Builds the float `float_idx` from its pending state and the lines
/// absorbed into it. `None` when it has neither a region nor a caption.
fn build_float(ctx: &PageContext, absorbed: &[Option<usize>], float_idx: usize, float: &PendingFloat) -> Option<Float> {
    let page = ctx.page;
    let caption: Vec<Line> = float
        .cap
        .map(|cap_idx| {
            ctx.captions[cap_idx]
                .lines
                .iter()
                .map(|&i| page.lines[i].clone())
                .collect()
        })
        .unwrap_or_default();
    let mut inner: Vec<&Line> = page
        .lines
        .iter()
        .enumerate()
        .filter(|(i, _)| absorbed[*i] == Some(float_idx))
        .map(|(_, line)| line)
        .collect();
    inner.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
    let inner_text = inner.iter().map(|line| line.text()).collect::<Vec<_>>().join(" ");
    let mut bbox = float.region.unwrap_or(Rect::EMPTY);
    if let Some(cap_idx) = float.cap {
        bbox = bbox.union(&ctx.captions[cap_idx].bbox);
    }
    if bbox.is_unset() {
        return None;
    }
    let (hrules, vrules, complex) = match float.region {
        Some(region) => inner_rules(page, &region, ctx.cfg),
        None => (Vec::new(), Vec::new(), false),
    };
    Some(Float {
        kind: float.kind,
        page: page.index,
        region: float.region,
        caption,
        label: float.cap.map(|cap_idx| ctx.captions[cap_idx].label.clone()),
        bbox,
        inner_text,
        inner_lines: inner.iter().map(|line| (*line).clone()).collect(),
        table_like: float.table_like,
        hrules,
        vrules,
        complex,
    })
}

/// The horizontal rules (with their line width) and vertical rules whose
/// centre is inside `region`, and whether the region is complex: it holds an
/// image, or more than a few paths that are neither rules nor cell shading.
fn inner_rules(page: &PageData, region: &Rect, cfg: &FloatsConfig) -> (Vec<(Rect, f32)>, Vec<Rect>, bool) {
    let (mut hrules, mut vrules) = (Vec::new(), Vec::new());
    let inside = |rect: &Rect| region.contains_point(rect.cx(), rect.cy());
    let mut complex = page.images.iter().any(|image| region.intersects(&image.bbox));
    let mut other = 0;
    for path in page.paths.iter().filter(|path| inside(&path.bbox)) {
        let white_fill = path.filled && !path.stroked && path.is_white();
        if path.is_hrule() && path.bbox.width() >= cfg.inner_rule_min_pt && !white_fill {
            hrules.push((path.bbox, path.line_width.max(path.bbox.height())));
        } else if path.is_vrule() && path.bbox.height() >= cfg.inner_rule_min_pt && !white_fill {
            vrules.push(path.bbox);
        } else if path.filled && !path.stroked && path.elements <= cfg.shading_max_elements_count {
            // Cell shading.
        } else {
            other += 1;
        }
    }
    complex |= other > cfg.complex_other_paths_count;
    (hrules, vrules, complex)
}

/// Removes the caption lines and the lines absorbed into a float from the
/// text flow.
fn remove_float_lines(lines: &mut Vec<Line>, is_caption_line: &[bool], absorbed: &[Option<usize>]) {
    let mut i = 0;
    lines.retain(|_| {
        let keep = !is_caption_line[i] && absorbed[i].is_none();
        i += 1;
        keep
    });
}
