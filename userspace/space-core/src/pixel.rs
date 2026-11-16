//! ARGB8888 / XRGB8888 color representation and abstract 2D canvas traits.

use crate::geometry::Rect;
use crate::simd;

/// 32-bit ARGB color (`0xAARRGGBB` in native u32).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct Color(pub u32);

impl Color {
    pub const TRANSPARENT: Self = Self(0x0000_0000);
    pub const BLACK: Self = Self(0xFF00_0000);
    pub const WHITE: Self = Self(0xFFFF_FFFF);

    #[inline]
    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Self {
        Self(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::argb(255, r, g, b)
    }

    #[inline]
    pub const fn alpha(self) -> u8 {
        (self.0 >> 24) as u8
    }

    #[inline]
    pub const fn red(self) -> u8 {
        (self.0 >> 16) as u8
    }

    #[inline]
    pub const fn green(self) -> u8 {
        (self.0 >> 8) as u8
    }

    #[inline]
    pub const fn blue(self) -> u8 {
        self.0 as u8
    }

    #[inline]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self((self.0 & 0x00FF_FFFF) | ((a as u32) << 24))
    }
}

/// Abstract 2D pixel target (Dependency Inversion for renderers and widgets).
pub trait Canvas {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn stride_pixels(&self) -> u32;
    fn pixels_mut(&mut self) -> &mut [u32];
    fn pixels(&self) -> &[u32];

    #[inline]
    fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width() as i32, self.height() as i32)
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        let clipped = rect.intersect(self.bounds());
        if clipped.is_empty() {
            return;
        }
        let stride = self.stride_pixels() as usize;
        let alpha = color.alpha();
        let buf = self.pixels_mut();

        if alpha == 255 {
            for row in clipped.y..clipped.bottom() {
                let start = row as usize * stride + clipped.x as usize;
                let end = start + clipped.w as usize;
                simd::fill_u32(&mut buf[start..end], color.0);
            }
        } else if alpha > 0 {
            for row in clipped.y..clipped.bottom() {
                let start = row as usize * stride + clipped.x as usize;
                let end = start + clipped.w as usize;
                simd::blend_solid_row(&mut buf[start..end], color.0);
            }
        }
    }

    fn draw_rect_outline(&mut self, rect: Rect, thickness: i32, color: Color) {
        if rect.is_empty() || thickness <= 0 {
            return;
        }
        let t = thickness.min(rect.w / 2).min(rect.h / 2).max(1);
        self.fill_rect(Rect::new(rect.x, rect.y, rect.w, t), color);
        self.fill_rect(Rect::new(rect.x, rect.bottom() - t, rect.w, t), color);
        self.fill_rect(Rect::new(rect.x, rect.y + t, t, rect.h - 2 * t), color);
        self.fill_rect(Rect::new(rect.right() - t, rect.y + t, t, rect.h - 2 * t), color);
    }

    fn fill_rounded_rect(&mut self, rect: Rect, radius: i32, color: Color) {
        let clipped = rect.intersect(self.bounds());
        if clipped.is_empty() || color.alpha() == 0 {
            return;
        }
        let r = radius.min(rect.w / 2).min(rect.h / 2).max(0);
        if r <= 1 {
            self.fill_rect(clipped, color);
            return;
        }
        let r_sq = r * r;
        let stride = self.stride_pixels() as usize;
        let buf = self.pixels_mut();
        let solid = color.alpha() == 255;

        for py in clipped.y..clipped.bottom() {
            let dy = if py < rect.y + r {
                (rect.y + r) - py
            } else if py >= rect.bottom() - r {
                py - (rect.bottom() - r - 1)
            } else {
                0
            };

            let inset = if dy == 0 {
                0
            } else {
                let dx_max = ((r_sq - dy * dy) as f32).sqrt() as i32;
                (r - dx_max).max(0)
            };

            let x_start = (rect.x + inset).max(clipped.x);
            let x_end = (rect.right() - inset).min(clipped.right());
            if x_end > x_start {
                let start = py as usize * stride + x_start as usize;
                let end = py as usize * stride + x_end as usize;
                if solid {
                    simd::fill_u32(&mut buf[start..end], color.0);
                } else {
                    simd::blend_solid_row(&mut buf[start..end], color.0);
                }
            }
        }
    }

    /// Blits a source pixel buffer region onto `self` at `(dst_x, dst_y)`,
    /// clipped to `clip_rect`, using SIMD alpha blending when `alpha_blend` is true.
    fn blit_from(
        &mut self,
        src: &[u32],
        src_stride: u32,
        src_rect: Rect,
        dst_x: i32,
        dst_y: i32,
        clip_rect: Rect,
        alpha_blend: bool,
    ) {
        let dst_rect = Rect::new(dst_x, dst_y, src_rect.w, src_rect.h)
            .intersect(clip_rect)
            .intersect(self.bounds());
        if dst_rect.is_empty() {
            return;
        }

        let off_x = dst_rect.x - dst_x;
        let off_y = dst_rect.y - dst_y;
        let dst_stride = self.stride_pixels() as usize;
        let s_stride = src_stride as usize;
        let dst_buf = self.pixels_mut();
        let w = dst_rect.w as usize;

        for row in 0..dst_rect.h {
            let sy = (src_rect.y + off_y + row) as usize;
            let sx = (src_rect.x + off_x) as usize;
            let dy = (dst_rect.y + row) as usize;
            let dx = dst_rect.x as usize;

            let s_idx = sy * s_stride + sx;
            let d_idx = dy * dst_stride + dx;
            if s_idx + w <= src.len() && d_idx + w <= dst_buf.len() {
                if alpha_blend {
                    simd::alpha_blend_row(&mut dst_buf[d_idx..d_idx + w], &src[s_idx..s_idx + w]);
                } else {
                    simd::blit_opaque_row(&mut dst_buf[d_idx..d_idx + w], &src[s_idx..s_idx + w]);
                }
            }
        }
    }
}

/// Owned CPU-backed ARGB8888 pixel buffer.
#[derive(Debug, Clone)]
pub struct PixelBuffer {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u32>,
}

impl PixelBuffer {
    pub fn new(width: u32, height: u32, clear: Color) -> Self {
        let len = width as usize * height as usize;
        Self {
            width,
            height,
            data: vec![clear.0; len],
        }
    }

    pub fn resize(&mut self, width: u32, height: u32, clear: Color) {
        self.width = width;
        self.height = height;
        self.data.resize(width as usize * height as usize, clear.0);
        self.data.fill(clear.0);
    }
}

impl Canvas for PixelBuffer {
    #[inline]
    fn width(&self) -> u32 {
        self.width
    }
    #[inline]
    fn height(&self) -> u32 {
        self.height
    }
    #[inline]
    fn stride_pixels(&self) -> u32 {
        self.width
    }
    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.data
    }
    #[inline]
    fn pixels(&self) -> &[u32] {
        &self.data
    }
}
