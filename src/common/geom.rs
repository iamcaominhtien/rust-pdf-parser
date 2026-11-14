//! Small geometry helpers. All coordinates are in PDF points with the origin
//! at the top-left corner of the page (y grows downwards).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An axis-aligned rectangle in page coordinates (points, y downwards).
///
/// A rectangle with `x1 <= x0` or `y1 <= y0` is empty; [`Rect::EMPTY`] is
/// the inverted "nothing accumulated yet" value that [`Rect::union`] turns
/// into the first rectangle it is combined with.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Rect {
    /// Left edge.
    pub x0: f32,
    /// Top edge.
    pub y0: f32,
    /// Right edge.
    pub x1: f32,
    /// Bottom edge.
    pub y1: f32,
}

impl Rect {
    /// The neutral element of [`Rect::union`]: infinitely inverted, so the
    /// union with any rectangle is that rectangle. Use it to start
    /// accumulating a bounding box; [`Rect::is_unset`] recognises it.
    pub const EMPTY: Rect = Rect {
        x0: f32::INFINITY,
        y0: f32::INFINITY,
        x1: f32::NEG_INFINITY,
        y1: f32::NEG_INFINITY,
    };

    /// A rectangle from its left, top, right and bottom edges.
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// True when the rectangle has no area (zero or negative width or
    /// height). Degenerate boxes such as rules count as empty.
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// True for the "nothing accumulated yet" sentinel.
    pub fn is_unset(&self) -> bool {
        self.x1 < self.x0 || self.y1 < self.y0
    }

    /// Width, never negative (0 for inverted rectangles).
    pub fn width(&self) -> f32 {
        (self.x1 - self.x0).max(0.0)
    }

    /// Height, never negative (0 for inverted rectangles).
    pub fn height(&self) -> f32 {
        (self.y1 - self.y0).max(0.0)
    }

    /// Area, 0 for empty or inverted rectangles.
    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    /// Horizontal centre.
    pub fn cx(&self) -> f32 {
        (self.x0 + self.x1) * 0.5
    }

    /// Vertical centre.
    pub fn cy(&self) -> f32 {
        (self.y0 + self.y1) * 0.5
    }

    /// Smallest rectangle containing both rectangles.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// The common part of both rectangles; inverted (see
    /// [`Rect::is_empty`]) when they do not overlap.
    pub fn intersect(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    /// True when the interiors overlap (touching edges do not count).
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }

    /// True when `other` lies entirely inside `self` (edges may coincide).
    pub fn contains(&self, other: &Rect) -> bool {
        self.x0 <= other.x0 && self.y0 <= other.y0 && self.x1 >= other.x1 && self.y1 >= other.y1
    }

    /// True when the point lies inside or on the border.
    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    /// The rectangle grown by `dx` on the left and right and by `dy` at the
    /// top and bottom (negative values shrink it).
    pub fn inflate(&self, dx: f32, dy: f32) -> Rect {
        Rect {
            x0: self.x0 - dx,
            y0: self.y0 - dy,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
        }
    }

    /// Length of the horizontal overlap (negative = gap).
    pub fn x_overlap(&self, other: &Rect) -> f32 {
        self.x1.min(other.x1) - self.x0.max(other.x0)
    }

    /// Length of the vertical overlap (negative = gap).
    pub fn y_overlap(&self, other: &Rect) -> f32 {
        self.y1.min(other.y1) - self.y0.max(other.y0)
    }

    /// Fraction of `other`'s area that lies inside `self`.
    ///
    /// A degenerate `other` (no area, e.g. a rule) counts as fully covered
    /// when its centre lies inside `self`, and as not covered otherwise.
    pub fn coverage_of(&self, other: &Rect) -> f32 {
        let other_area = other.area();
        if other_area <= 0.0 {
            return if self.contains_point(other.cx(), other.cy()) {
                1.0
            } else {
                0.0
            };
        }
        let overlap = self.intersect(other);
        if overlap.is_empty() {
            0.0
        } else {
            overlap.area() / other_area
        }
    }
}

/// Median of a slice (returns `default` if empty). Sorts a copy.
///
/// For an even number of values this is the upper of the two middle values,
/// not their mean.
pub fn median(values: &[f32], default: f32) -> f32 {
    if values.is_empty() {
        return default;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    sorted[sorted.len() / 2]
}

/// Most frequent value after rounding to `step`, counting each value with its weight.
///
/// `values` yields `(value, weight)` pairs. Each value is rounded to the
/// nearest multiple of `step`, and the multiple with the largest total weight
/// is returned; on a tie the smaller value wins, so the result does not
/// depend on hash order. Returns `default` when `values` is empty.
pub fn weighted_mode(values: impl Iterator<Item = (f32, f32)>, step: f32, default: f32) -> f32 {
    let mut hist: HashMap<i32, f32> = HashMap::new();
    for (value, weight) in values {
        *hist.entry((value / step).round() as i32).or_insert(0.0) += weight;
    }
    hist.into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(bucket, _)| bucket as f32 * step)
        .unwrap_or(default)
}
