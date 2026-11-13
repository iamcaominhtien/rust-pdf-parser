//! Reading order.
//!
//! A page is cut into horizontal zones; a zone is either single-column or is
//! split at its gutters into columns, which are ordered recursively. The
//! result is a list of leaves (column fragments) in reading order, each made
//! of rows of items.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use super::floats::Float;
use super::page::DocStats;
use crate::common::geom::Rect;
use crate::model::Line;

/// Tuning parameters of reading order. Part of [`LayoutConfig`](crate::config::LayoutConfig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderConfig {
    /// Narrowest vertical strip of white space (in body sizes) that counts
    /// as a gutter between columns. Ordinary word spaces are far narrower,
    /// column gutters are usually one to two body sizes wide.
    pub min_gutter_em: f32,
    /// Top of a line's band above its baseline (in font sizes), roughly the
    /// x-height plus a little: tighter than the nominal box so that lines
    /// with tall glyphs do not bridge zones.
    pub line_band_ascent_em: f32,
    /// Bottom of a line's band below its baseline (in font sizes).
    pub line_band_descent_em: f32,
    /// How far (in points) a float's band is shrunk at the top and bottom,
    /// so that floats touching a neighbour do not fuse with it.
    pub float_band_inset_pt: f32,
    /// Smallest height (in points, measured from the float's top) of a
    /// float's band, so that very flat floats keep a non-empty band.
    pub float_band_min_height_pt: f32,
    /// Two line centres closer than this (in points) are pieces of one
    /// visual row when measuring how many rows a group of items has.
    pub same_row_distance_pt: f32,
    /// How many rows a float counts as when measuring the height of a group
    /// of items: a float is a substantial block even though it is one item.
    pub float_weight_rows_count: usize,
    /// Groups with fewer items are never split into columns.
    pub min_items_to_split_count: usize,
    /// Each side of a column split must be at least this fraction of the
    /// width of the whole group.
    pub min_column_width_ratio: f32,
    /// Each side of a column split must have at least this many rows (see
    /// [`same_row_distance_pt`](Self::same_row_distance_pt)). Two lines side
    /// by side are more often a line broken by a wide gap than columns.
    pub min_column_rows_count: usize,
    /// Fraction of the smaller height by which an item must overlap a row
    /// vertically to join it.
    pub row_overlap_ratio: f32,
    /// How far (in points) an item of a single-column zone may stick out of
    /// a column of the zone below and still be moved into that column.
    pub column_fit_tolerance_pt: f32,
    /// Deepest nesting of column splits; deeper groups are kept whole. A
    /// guard against pathological layouts rather than a layout property.
    pub max_split_depth_count: u32,
}

impl Default for OrderConfig {
    fn default() -> Self {
        Self {
            min_gutter_em: 0.7,
            line_band_ascent_em: 0.6,
            line_band_descent_em: 0.1,
            float_band_inset_pt: 1.0,
            float_band_min_height_pt: 1.5,
            same_row_distance_pt: 3.0,
            float_weight_rows_count: 3,
            min_items_to_split_count: 6,
            min_column_width_ratio: 0.15,
            min_column_rows_count: 3,
            row_overlap_ratio: 0.5,
            column_fit_tolerance_pt: 3.0,
            max_split_depth_count: 8,
        }
    }
}

/// Reference to an item of a page: a text line or a float.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemRef {
    /// Index into the page's lines.
    Line(usize),
    /// Index into the page's floats.
    Float(usize),
}

/// A column fragment of a page, the unit of reading order.
#[derive(Debug, Clone)]
pub struct Leaf {
    /// Index of the top-level zone on the page.
    pub zone: usize,
    /// Number of columns of that zone.
    pub zone_cols: usize,
    /// Column of the zone this leaf belongs to.
    pub col: usize,
    /// The items of the leaf, grouped into rows; rows top to bottom, items
    /// of a row left to right.
    pub rows: Vec<Vec<ItemRef>>,
    /// Union of the boxes of the leaf's items.
    pub bbox: Rect,
}

/// An item to be ordered, with the geometry the ordering works on.
#[derive(Debug, Clone, Copy)]
struct OrderItem {
    /// The line or float this item stands for.
    item: ItemRef,
    /// Box of the line or float.
    bbox: Rect,
    /// Top of the vertical extent used for banding (tighter than the box).
    y0: f32,
    /// Bottom of the vertical extent used for banding.
    y1: f32,
}

/// The result of cutting a group of items into zones: either a run of
/// single-column zones or a zone split into columns.
enum Part {
    /// Items read top to bottom as one column.
    Leaf(Vec<OrderItem>),
    /// Columns, left to right, each to be ordered on its own.
    Split(Vec<Vec<OrderItem>>),
}

/// Zone and column splitting with the thresholds of one page.
struct Splitter<'cfg> {
    /// The thresholds.
    cfg: &'cfg OrderConfig,
    /// Narrowest gutter, in points, for this document's body size.
    min_gutter: f32,
}

impl Splitter<'_> {
    /// Splits a zone at its widest valid gutter (recursively), returning the
    /// columns from left to right. A single column means "do not split".
    fn columns(&self, items: Vec<OrderItem>) -> Vec<Vec<OrderItem>> {
        if items.len() < self.cfg.min_items_to_split_count {
            return vec![items];
        }
        let whole = extent(&items);
        let mut candidates = gutters(&items, self.min_gutter);
        candidates.sort_by(|a, b| (b.1 - b.0).total_cmp(&(a.1 - a.0)));
        for (gap_start, gap_end) in candidates {
            let mid = (gap_start + gap_end) * 0.5;
            let (left, right): (Vec<OrderItem>, Vec<OrderItem>) = items.iter().partition(|item| item.bbox.cx() < mid);
            let (left_extent, right_extent) = (extent(&left), extent(&right));
            let wide_enough = left_extent.width() >= self.cfg.min_column_width_ratio * whole.width()
                && right_extent.width() >= self.cfg.min_column_width_ratio * whole.width();
            // Two lines side by side are more often an accident (a line broken
            // by a wide gap) than a column layout.
            if weight(&left, self.cfg) >= self.cfg.min_column_rows_count
                && weight(&right, self.cfg) >= self.cfg.min_column_rows_count
                && wide_enough
            {
                let mut out = self.columns(left);
                out.extend(self.columns(right));
                return out;
            }
        }
        vec![items]
    }

    /// Groups consecutive bands into zones. A zone is either a run of bands
    /// without a gutter, or a run of bands that together still have a common
    /// gutter.
    fn zones(&self, items: Vec<OrderItem>) -> Vec<Vec<OrderItem>> {
        let mut zones: Vec<Vec<OrderItem>> = Vec::new();
        let mut cur_has_gutter = false;
        for band in bands(items) {
            let band_gutter = !gutters(&band, self.min_gutter).is_empty();
            match zones.last_mut() {
                None => {
                    cur_has_gutter = band_gutter;
                    zones.push(band);
                }
                Some(zone) => {
                    if cur_has_gutter {
                        let mut merged = zone.clone();
                        merged.extend(band.iter().copied());
                        if !gutters(&merged, self.min_gutter).is_empty() {
                            *zone = merged;
                        } else {
                            cur_has_gutter = band_gutter;
                            zones.push(band);
                        }
                    } else if band_gutter {
                        cur_has_gutter = true;
                        zones.push(band);
                    } else {
                        zone.extend(band);
                    }
                }
            }
        }
        zones
    }

    /// Cuts a group of items into zones and splits each zone into columns.
    /// Consecutive single-column zones are merged into one [`Part::Leaf`].
    fn partition(&self, items: Vec<OrderItem>) -> Vec<Part> {
        let mut zones = self.zones(items);
        self.attach_zones_above_columns(&mut zones);

        let mut parts: Vec<Part> = Vec::new();
        for zone in zones {
            let cols = self.columns(zone);
            if cols.len() == 1 {
                let items = cols.into_iter().next().unwrap();
                if let Some(Part::Leaf(prev)) = parts.last_mut() {
                    prev.extend(items);
                } else {
                    parts.push(Part::Leaf(items));
                }
            } else {
                parts.push(Part::Split(cols));
            }
        }
        parts
    }

    /// A single-column zone directly above a multi-column zone belongs to it
    /// when none of its items crosses the gutter (e.g. a heading on top of the
    /// left column). Merges such zones into the zone below, bottom-up.
    fn attach_zones_above_columns(&self, zones: &mut Vec<Vec<OrderItem>>) {
        let mut i = zones.len();
        while i >= 2 {
            i -= 1;
            let cols = self.columns(zones[i].clone());
            if cols.len() < 2 {
                continue;
            }
            let prev_cols = self.columns(zones[i - 1].clone());
            if prev_cols.len() >= 2 {
                continue;
            }
            let bounds: Vec<Rect> = cols.iter().map(|column| extent(column)).collect();
            let tolerance = self.cfg.column_fit_tolerance_pt;
            let fits = zones[i - 1].iter().all(|item| {
                bounds
                    .iter()
                    .any(|bound| item.bbox.x0 >= bound.x0 - tolerance && item.bbox.x1 <= bound.x1 + tolerance)
            });
            if fits {
                let prev = zones.remove(i - 1);
                zones[i - 1].splice(0..0, prev);
            }
        }
    }

    /// Recursively partitions `items`, appending the resulting leaves (in
    /// reading order) to `out`. Beyond the maximum depth the items are kept
    /// as one leaf.
    fn flatten(&self, items: Vec<OrderItem>, depth: u32, out: &mut Vec<Vec<OrderItem>>) {
        if depth > self.cfg.max_split_depth_count {
            out.push(items);
            return;
        }
        for part in self.partition(items) {
            match part {
                Part::Leaf(items) => out.push(items),
                Part::Split(cols) => {
                    for column in cols {
                        self.flatten(column, depth + 1, out);
                    }
                }
            }
        }
    }
}

/// Orders the lines and floats of a page.
///
/// Returns the leaves (column fragments) in reading order: top-level zones
/// from top to bottom, and inside a multi-column zone its columns from left
/// to right, each possibly split again. Every line and float of the page is
/// in exactly one leaf.
pub fn order_page(lines: &[Line], floats: &[Float], stats: &DocStats, cfg: &OrderConfig) -> Vec<Leaf> {
    let items = collect_items(lines, floats, cfg);
    let splitter = Splitter {
        cfg,
        min_gutter: cfg.min_gutter_em * stats.body_size,
    };

    let mut leaves = Vec::new();
    for (zone, part) in splitter.partition(items).into_iter().enumerate() {
        match part {
            Part::Leaf(items) => {
                let bbox = extent(&items);
                leaves.push(Leaf {
                    zone,
                    zone_cols: 1,
                    col: 0,
                    rows: rows(items, cfg),
                    bbox,
                });
            }
            Part::Split(cols) => {
                let n = cols.len();
                for (col, column) in cols.into_iter().enumerate() {
                    let mut sub = Vec::new();
                    splitter.flatten(column, 1, &mut sub);
                    for items in sub {
                        let bbox = extent(&items);
                        leaves.push(Leaf {
                            zone,
                            zone_cols: n,
                            col,
                            rows: rows(items, cfg),
                            bbox,
                        });
                    }
                }
            }
        }
    }
    leaves
}

/// Turns the lines and floats of a page into items, lines first. Each item
/// gets the vertical band used to cut the page into zones.
fn collect_items(lines: &[Line], floats: &[Float], cfg: &OrderConfig) -> Vec<OrderItem> {
    let mut items: Vec<OrderItem> = Vec::with_capacity(lines.len() + floats.len());
    for (i, line) in lines.iter().enumerate() {
        items.push(OrderItem {
            item: ItemRef::Line(i),
            bbox: line.bbox,
            y0: line.baseline - cfg.line_band_ascent_em * line.size,
            y1: line.baseline + cfg.line_band_descent_em * line.size,
        });
    }
    for (i, float) in floats.iter().enumerate() {
        items.push(OrderItem {
            item: ItemRef::Float(i),
            bbox: float.bbox,
            y0: float.bbox.y0 + cfg.float_band_inset_pt,
            y1: (float.bbox.y1 - cfg.float_band_inset_pt).max(float.bbox.y0 + cfg.float_band_min_height_pt),
        });
    }
    items
}

/// Horizontal extents of the items, merged where they overlap, left to right.
fn x_intervals(items: &[OrderItem]) -> Vec<(f32, f32)> {
    let mut intervals: Vec<(f32, f32)> = items.iter().map(|item| (item.bbox.x0, item.bbox.x1)).collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f32, f32)> = Vec::new();
    for (start, end) in intervals {
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// The vertical strips free of items that are at least `min_gutter` points
/// wide, as `(left, right)` pairs from left to right. Strips at the outer
/// edges are not gutters.
fn gutters(items: &[OrderItem], min_gutter: f32) -> Vec<(f32, f32)> {
    let intervals = x_intervals(items);
    intervals
        .windows(2)
        .filter(|pair| pair[1].0 - pair[0].1 >= min_gutter)
        .map(|pair| (pair[0].1, pair[1].0))
        .collect()
}

/// "Height" of a group of items in rows: distinct text rows plus a fixed
/// number per float. Pieces of one stretched-out line count once.
fn weight(items: &[OrderItem], cfg: &OrderConfig) -> usize {
    let mut ys: Vec<f32> = Vec::new();
    let mut floats = 0;
    for item in items {
        match item.item {
            ItemRef::Line(_) => ys.push(item.bbox.cy()),
            ItemRef::Float(_) => floats += 1,
        }
    }
    ys.sort_by(|a, b| a.total_cmp(b));
    let mut rows = 0;
    let mut last = f32::NEG_INFINITY;
    for y in ys {
        if y - last > cfg.same_row_distance_pt {
            rows += 1;
        }
        last = y;
    }
    rows + cfg.float_weight_rows_count * floats
}

/// Union of the boxes of the items.
fn extent(items: &[OrderItem]) -> Rect {
    items.iter().fold(Rect::EMPTY, |acc, item| acc.union(&item.bbox))
}

/// Cuts the items into horizontal bands: maximal groups whose vertical
/// extents overlap (transitively), from top to bottom.
fn bands(mut items: Vec<OrderItem>) -> Vec<Vec<OrderItem>> {
    items.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let mut out: Vec<Vec<OrderItem>> = Vec::new();
    let mut cur_y1 = f32::NEG_INFINITY;
    for item in items {
        if out.is_empty() || item.y0 >= cur_y1 {
            out.push(vec![item]);
            cur_y1 = item.y1;
        } else {
            cur_y1 = cur_y1.max(item.y1);
            out.last_mut().unwrap().push(item);
        }
    }
    out
}

/// Groups the items of a leaf into rows (top to bottom, left to right).
/// A float always forms a row of its own.
fn rows(mut items: Vec<OrderItem>, cfg: &OrderConfig) -> Vec<Vec<ItemRef>> {
    items.sort_by(|a, b| a.bbox.cy().total_cmp(&b.bbox.cy()));
    let mut out: Vec<(Rect, Vec<OrderItem>)> = Vec::new();
    for item in items {
        let joined = match out.last_mut() {
            Some((row_box, row)) => {
                let overlap = row_box.y_overlap(&item.bbox);
                let is_float = matches!(item.item, ItemRef::Float(_))
                    || row.iter().any(|member| matches!(member.item, ItemRef::Float(_)));
                if !is_float && overlap >= cfg.row_overlap_ratio * row_box.height().min(item.bbox.height()) {
                    *row_box = row_box.union(&item.bbox);
                    row.push(item);
                    true
                } else {
                    false
                }
            }
            None => false,
        };
        if !joined {
            out.push((item.bbox, vec![item]));
        }
    }
    out.sort_by(|a, b| a.0.y0.total_cmp(&b.0.y0));
    out.into_iter()
        .map(|(_, mut row)| {
            row.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
            row.into_iter().map(|member| member.item).collect()
        })
        .collect()
}
