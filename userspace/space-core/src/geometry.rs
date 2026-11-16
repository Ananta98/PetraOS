//! Cache-friendly geometry primitives and damage-rectangle coalescing.

/// 2D integer point in screen or surface coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    #[inline]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// 2D unsigned extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct Size {
    pub w: u32,
    pub h: u32,
}

impl Size {
    #[inline]
    pub const fn new(w: u32, h: u32) -> Self {
        Self { w, h }
    }
}

/// Axis-aligned integer rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const EMPTY: Self = Self { x: 0, y: 0, w: 0, h: 0 };

    #[inline]
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    #[inline]
    pub const fn right(self) -> i32 {
        self.x + self.w
    }

    #[inline]
    pub const fn bottom(self) -> i32 {
        self.y + self.h
    }

    #[inline]
    pub const fn area(self) -> i64 {
        if self.is_empty() {
            0
        } else {
            self.w as i64 * self.h as i64
        }
    }

    #[inline]
    pub const fn contains(self, p: Point) -> bool {
        !self.is_empty()
            && p.x >= self.x
            && p.x < self.right()
            && p.y >= self.y
            && p.y < self.bottom()
    }

    #[inline]
    pub fn intersect(self, other: Self) -> Self {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        if x2 <= x1 || y2 <= y1 {
            Self::EMPTY
        } else {
            Self::new(x1, y1, x2 - x1, y2 - y1)
        }
    }

    #[inline]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = self.right().max(other.right());
        let y2 = self.bottom().max(other.bottom());
        Self::new(x1, y1, x2 - x1, y2 - y1)
    }

    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        !self.intersect(other).is_empty()
    }
}

/// Contiguous, cache-friendly list of dirty rectangles with automatic coalescing.
///
/// Limits fragmentation by merging overlapping or near-adjacent rectangles
/// so the compositor redraws only damaged screen regions.
#[derive(Debug, Clone, Default)]
pub struct DamageRegion {
    rects: Vec<Rect>,
}

impl DamageRegion {
    const MAX_RECTS: usize = 32;

    pub fn new() -> Self {
        Self {
            rects: Vec::with_capacity(16),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.rects.clear();
    }

    #[inline]
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// Adds a damaged rectangle, clipping to `bounds` and merging if overlapping.
    pub fn add_clipped(&mut self, rect: Rect, bounds: Rect) {
        let r = rect.intersect(bounds);
        if r.is_empty() {
            return;
        }
        self.add(r);
    }

    /// Adds a damaged rectangle, coalescing with existing regions when beneficial.
    pub fn add(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }

        // Merge with any existing rect if union area <= sum of areas + small slack.
        for existing in &mut self.rects {
            if existing.intersects(rect) {
                let u = existing.union(rect);
                if u.area() <= existing.area() + rect.area() + 256 {
                    *existing = u;
                    return;
                }
            }
        }

        if self.rects.len() >= Self::MAX_RECTS {
            let mut merged = rect;
            for r in self.rects.drain(..) {
                merged = merged.union(r);
            }
            self.rects.push(merged);
        } else {
            self.rects.push(rect);
        }
    }

    /// Computes the bounding box of all damaged rectangles.
    pub fn bounding_box(&self) -> Rect {
        let mut acc = Rect::EMPTY;
        for &r in &self.rects {
            acc = acc.union(r);
        }
        acc
    }
}
