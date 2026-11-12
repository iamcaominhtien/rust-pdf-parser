//! Small geometry helpers. All coordinates are in PDF points with the origin
//! at the top-left corner of the page (y grows downwards).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub const EMPTY: Rect = Rect {
        x0: f32::INFINITY,
        y0: f32::INFINITY,
        x1: f32::NEG_INFINITY,
        y1: f32::NEG_INFINITY,
    };

    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// True for the "nothing accumulated yet" sentinel.
    pub fn is_unset(&self) -> bool {
        self.x1 < self.x0 || self.y1 < self.y0
    }

    pub fn width(&self) -> f32 {
        (self.x1 - self.x0).max(0.0)
    }

    pub fn height(&self) -> f32 {
        (self.y1 - self.y0).max(0.0)
    }

    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    pub fn cx(&self) -> f32 {
        (self.x0 + self.x1) * 0.5
    }

    pub fn cy(&self) -> f32 {
        (self.y0 + self.y1) * 0.5
    }

    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        }
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }

    pub fn contains(&self, o: &Rect) -> bool {
        self.x0 <= o.x0 && self.y0 <= o.y0 && self.x1 >= o.x1 && self.y1 >= o.y1
    }

    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    pub fn inflate(&self, dx: f32, dy: f32) -> Rect {
        Rect {
            x0: self.x0 - dx,
            y0: self.y0 - dy,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
        }
    }

    /// Length of the horizontal overlap (negative = gap).
    pub fn x_overlap(&self, o: &Rect) -> f32 {
        self.x1.min(o.x1) - self.x0.max(o.x0)
    }

    /// Length of the vertical overlap (negative = gap).
    pub fn y_overlap(&self, o: &Rect) -> f32 {
        self.y1.min(o.y1) - self.y0.max(o.y0)
    }

    /// Fraction of `o`'s area that lies inside `self`.
    pub fn coverage_of(&self, o: &Rect) -> f32 {
        let a = o.area();
        if a <= 0.0 {
            return if self.contains_point(o.cx(), o.cy()) { 1.0 } else { 0.0 };
        }
        let i = self.intersect(o);
        if i.is_empty() { 0.0 } else { i.area() / a }
    }
}

/// Median of a slice (returns `default` if empty). Sorts a copy.
pub fn median(values: &[f32], default: f32) -> f32 {
    if values.is_empty() {
        return default;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Most frequent value after rounding to `step`, weighted by `weights`.
pub fn weighted_mode(values: impl Iterator<Item = (f32, f32)>, step: f32, default: f32) -> f32 {
    let mut hist: std::collections::HashMap<i32, f32> = std::collections::HashMap::new();
    for (v, w) in values {
        *hist.entry((v / step).round() as i32).or_insert(0.0) += w;
    }
    hist.into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(k, _)| k as f32 * step)
        .unwrap_or(default)
}
