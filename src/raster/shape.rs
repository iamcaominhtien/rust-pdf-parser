//! Pieces of ink and their normalised shapes.

/// Side of the grid a shape is normalised to.
const GRID: usize = 20;

/// A bitmap of ink placed on a pixel grid.
#[derive(Debug, Clone)]
pub struct Mask {
    /// How much of each pixel is ink (0 to 1), row by row.
    pub ink: Vec<f32>,
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Position of the left edge.
    pub origin_x: f32,
    /// Position of the top edge.
    pub origin_y: f32,
}

impl Mask {
    /// A mask of `ink` (coverage, row by row), `width` wide and `height`
    /// high, with its top-left corner at (`origin_x`, `origin_y`).
    pub fn new(ink: Vec<f32>, width: usize, height: usize, origin_x: f32, origin_y: f32) -> Self {
        Self {
            ink,
            width,
            height,
            origin_x,
            origin_y,
        }
    }

    /// Whether the pixel at column `x`, row `y` is ink.
    pub fn at(&self, x: usize, y: usize) -> bool {
        self.cover(x, y) > 0.0
    }

    /// How much of the pixel at column `x`, row `y` is ink.
    pub fn cover(&self, x: usize, y: usize) -> f32 {
        if x < self.width && y < self.height {
            self.ink[y * self.width + x]
        } else {
            0.0
        }
    }

    /// The mask cut to its ink, or `None` when it has none.
    pub fn cropped(&self) -> Option<Self> {
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for y in 0..self.height {
            for x in 0..self.width {
                if self.at(x, y) {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        if x1 <= x0 {
            return None;
        }
        let (width, height) = (x1 - x0, y1 - y0);
        let ink = (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .map(|(x, y)| self.cover(x, y))
            .collect();
        Some(Self::new(
            ink,
            width,
            height,
            self.origin_x + x0 as f32,
            self.origin_y + y0 as f32,
        ))
    }

    /// The separate pieces of ink (8-connected), each as the list of its
    /// pixels (column, row).
    pub fn pieces(&self) -> Vec<Vec<(usize, usize)>> {
        let mut seen = vec![false; self.ink.len()];
        let mut pieces = Vec::new();
        for start in 0..self.ink.len() {
            if self.ink[start] <= 0.0 || seen[start] {
                continue;
            }
            seen[start] = true;
            let mut stack = vec![start];
            let mut piece = Vec::new();
            while let Some(idx) = stack.pop() {
                let (x, y) = (idx % self.width, idx / self.width);
                piece.push((x, y));
                for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                    let (nx, ny) = (x as isize + dx, y as isize + dy);
                    if nx < 0 || ny < 0 || nx as usize >= self.width || ny as usize >= self.height {
                        continue;
                    }
                    let next = ny as usize * self.width + nx as usize;
                    if self.ink[next] > 0.0 && !seen[next] {
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
            pieces.push(piece);
        }
        pieces
    }

    /// Number of separate pieces of ink.
    pub fn parts(&self) -> usize {
        self.pieces().len()
    }
}

/// The shape of a piece of ink, scaled to a square grid (whatever its
/// proportions, which are kept apart).
#[derive(Debug, Clone)]
pub struct Shape {
    /// Blurred ink of the grid, centred on its mean and scaled to unit
    /// length (or only scaled, for a uniform grid).
    grid: Vec<f32>,
    /// Width over height of the ink.
    pub aspect: f32,
}

/// A piece narrower than this fraction of its height (or the converse)
/// is a line.
const THIN_MAX_ASPECT: f32 = 0.1;

/// Samples per grid cell along each axis.
const SAMPLES: usize = 4;

impl Shape {
    /// The shape of `mask`.
    pub fn of(mask: &Mask) -> Self {
        let mut grid = vec![0.0f32; GRID * GRID];
        for (cell, value) in grid.iter_mut().enumerate() {
            let (cx, cy) = (cell % GRID, cell / GRID);
            let mut hits = 0.0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let fx = (cx * SAMPLES + sx) as f32 + 0.5;
                    let fy = (cy * SAMPLES + sy) as f32 + 0.5;
                    let x = (fx / (GRID * SAMPLES) as f32 * mask.width as f32) as usize;
                    let y = (fy / (GRID * SAMPLES) as f32 * mask.height as f32) as usize;
                    hits += mask.cover(x, y);
                }
            }
            *value = hits / (SAMPLES * SAMPLES) as f32;
        }
        let blurred: Vec<f32> = (0..GRID * GRID)
            .map(|cell| {
                let (cx, cy) = ((cell % GRID) as isize, (cell / GRID) as isize);
                let mut sum = 0.0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (x, y) = (
                            (cx + dx).clamp(0, GRID as isize - 1),
                            (cy + dy).clamp(0, GRID as isize - 1),
                        );
                        sum += grid[y as usize * GRID + x as usize];
                    }
                }
                sum / 9.0
            })
            .collect();
        let mean = blurred.iter().sum::<f32>() / blurred.len() as f32;
        let centred: Vec<f32> = blurred.iter().map(|value| value - mean).collect();
        let norm = centred.iter().map(|value| value * value).sum::<f32>().sqrt();
        // A long thin piece (a bar, a rule) has no shape to speak of at this
        // resolution, only proportions.
        let aspect = mask.width as f32 / mask.height.max(1) as f32;
        let thin = !(THIN_MAX_ASPECT..=1.0 / THIN_MAX_ASPECT).contains(&aspect);
        let grid = if norm > 1e-3 && !thin {
            centred.iter().map(|value| value / norm).collect()
        } else {
            vec![0.0; GRID * GRID]
        };
        Self { grid, aspect }
    }

    /// How alike two shapes are, from -1 to 1 (their correlation); 1 for two
    /// uniform shapes (bars, which only their proportions tell apart).
    pub fn likeness(&self, other: &Shape) -> f32 {
        let uniform = |shape: &Shape| shape.grid.iter().all(|value| *value == 0.0);
        if uniform(self) || uniform(other) {
            return if uniform(self) && uniform(other) { 1.0 } else { 0.0 };
        }
        self.grid.iter().zip(&other.grid).map(|(a, b)| a * b).sum()
    }
}
