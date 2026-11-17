//! Declarative BoxLayout engine (SerenityOS `LibGUI::BoxLayout` pattern).
//!
//! Computes child bounds inside a container `Rect` using padding, spacing, and
//! `Fixed` / `Expand` size policies so apps resize cleanly without hardcoded math.

use crate::traits::Layoutable;
use space_core::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizePolicy {
    Fixed(i32),
    Expand,
}

#[derive(Debug, Clone, Copy)]
pub struct Insets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Insets {
    pub const ZERO: Self = Self {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    pub const fn all(v: i32) -> Self {
        Self {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }

    pub const fn vh(v: i32, h: i32) -> Self {
        Self {
            left: h,
            top: v,
            right: h,
            bottom: v,
        }
    }

    pub fn apply(self, rect: Rect) -> Rect {
        Rect::new(
            rect.x + self.left,
            rect.y + self.top,
            (rect.w - self.left - self.right).max(0),
            (rect.h - self.top - self.bottom).max(0),
        )
    }
}

/// Vertical or horizontal linear layout pass over `Layoutable` items.
pub struct BoxLayout {
    pub vertical: bool,
    pub gap: i32,
    pub insets: Insets,
}

impl BoxLayout {
    pub const fn vbox(gap: i32, insets: Insets) -> Self {
        Self {
            vertical: true,
            gap,
            insets,
        }
    }

    pub const fn hbox(gap: i32, insets: Insets) -> Self {
        Self {
            vertical: false,
            gap,
            insets,
        }
    }

    /// Computes child `Rect`s for `policies` inside `container`.
    pub fn compute(&self, container: Rect, policies: &[SizePolicy]) -> Vec<Rect> {
        let inner = self.insets.apply(container);
        let n = policies.len();
        if n == 0 {
            return Vec::new();
        }

        let total_main = if self.vertical { inner.h } else { inner.w };
        let total_gaps = self.gap * (n as i32 - 1).max(0);
        let mut fixed_sum = 0i32;
        let mut expand_count = 0i32;

        for &p in policies {
            match p {
                SizePolicy::Fixed(sz) => fixed_sum += sz.max(0),
                SizePolicy::Expand => expand_count += 1,
            }
        }

        let rem = (total_main - total_gaps - fixed_sum).max(0);
        let expand_sz = if expand_count > 0 { rem / expand_count } else { 0 };
        let mut extra = if expand_count > 0 { rem % expand_count } else { 0 };

        let mut out = Vec::with_capacity(n);
        let mut cursor = if self.vertical { inner.y } else { inner.x };

        for &p in policies {
            let sz = match p {
                SizePolicy::Fixed(v) => v.max(0),
                SizePolicy::Expand => {
                    let bonus = if extra > 0 {
                        extra -= 1;
                        1
                    } else {
                        0
                    };
                    expand_sz + bonus
                }
            };
            let r = if self.vertical {
                Rect::new(inner.x, cursor, inner.w, sz)
            } else {
                Rect::new(cursor, inner.y, sz, inner.h)
            };
            out.push(r);
            cursor += sz + self.gap;
        }
        out
    }

    /// Applies computed bounds directly to mutable `Layoutable` targets.
    pub fn apply(&self, container: Rect, items: &mut [(&mut dyn Layoutable, SizePolicy)]) {
        let policies: Vec<SizePolicy> = items.iter().map(|(_, p)| *p).collect();
        let rects = self.compute(container, &policies);
        for ((item, _), r) in items.iter_mut().zip(rects) {
            item.set_bounds(r);
        }
    }
}
