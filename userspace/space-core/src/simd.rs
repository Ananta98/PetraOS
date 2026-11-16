//! SIMD-accelerated pixel operations (SSE2 on x86_64 with scalar fallback).
//!
//! Processes 4 pixels (16 bytes) per vector instruction for solid fills,
//! opaque blits, and Porter-Duff `SOURCE_OVER` alpha blending.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// Fills `dst` with `pixel` using SSE2 128-bit stores when available.
#[inline]
pub fn fill_u32(dst: &mut [u32], pixel: u32) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse2") {
            unsafe {
                fill_u32_sse2(dst, pixel);
            }
            return;
        }
    }
    dst.fill(pixel);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn fill_u32_sse2(dst: &mut [u32], pixel: u32) {
    let len = dst.len();
    let ptr = dst.as_mut_ptr();
    let vec = _mm_set1_epi32(pixel as i32);
    let mut i = 0;
    while i + 4 <= len {
        _mm_storeu_si128(ptr.add(i) as *mut __m128i, vec);
        i += 4;
    }
    while i < len {
        *ptr.add(i) = pixel;
        i += 1;
    }
}

/// Copies `src` to `dst` using fast block copy.
#[inline]
pub fn blit_opaque_row(dst: &mut [u32], src: &[u32]) {
    debug_assert_eq!(dst.len(), src.len());
    dst.copy_from_slice(src);
}

/// Blends a solid ARGB color `src_pixel` over `dst` using `SOURCE_OVER`.
#[inline]
pub fn blend_solid_row(dst: &mut [u32], src_pixel: u32) {
    let a = (src_pixel >> 24) & 0xFF;
    if a == 0 {
        return;
    }
    if a == 255 {
        fill_u32(dst, src_pixel);
        return;
    }
    let inv_a = 255 - a;
    let sr = ((src_pixel >> 16) & 0xFF) * a;
    let sg = ((src_pixel >> 8) & 0xFF) * a;
    let sb = (src_pixel & 0xFF) * a;

    for d in dst.iter_mut() {
        let dp = *d;
        let dr = ((dp >> 16) & 0xFF) * inv_a + sr;
        let dg = ((dp >> 8) & 0xFF) * inv_a + sg;
        let db = (dp & 0xFF) * inv_a + sb;
        *d = 0xFF00_0000 | ((dr / 255) << 16) | ((dg / 255) << 8) | (db / 255);
    }
}

/// Alpha-blends `src` over `dst` (`SOURCE_OVER`) using SSE2 for 2 pixels at a time.
#[inline]
pub fn alpha_blend_row(dst: &mut [u32], src: &[u32]) {
    debug_assert_eq!(dst.len(), src.len());
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse2") {
            unsafe {
                alpha_blend_row_sse2(dst, src);
            }
            return;
        }
    }
    alpha_blend_row_scalar(dst, src);
}

#[inline]
fn alpha_blend_row_scalar(dst: &mut [u32], src: &[u32]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        let a = (s >> 24) & 0xFF;
        if a == 0 {
            continue;
        }
        if a == 255 {
            *d = s;
            continue;
        }
        let inv_a = 255 - a;
        let dp = *d;
        let r = (((s >> 16) & 0xFF) * a + ((dp >> 16) & 0xFF) * inv_a) / 255;
        let g = (((s >> 8) & 0xFF) * a + ((dp >> 8) & 0xFF) * inv_a) / 255;
        let b = ((s & 0xFF) * a + (dp & 0xFF) * inv_a) / 255;
        *d = 0xFF00_0000 | (r << 16) | (g << 8) | b;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn alpha_blend_row_sse2(dst: &mut [u32], src: &[u32]) {
    let len = dst.len();
    let d_ptr = dst.as_mut_ptr();
    let s_ptr = src.as_ptr();
    let zero = _mm_setzero_si128();
    let ff = _mm_set1_epi16(255);
    let round_bias = _mm_set1_epi16(128);
    let opaque_alpha = _mm_set1_epi32(0xFF00_0000u32 as i32);

    let mut i = 0;
    while i + 2 <= len {
        // Fast-path check for 2 opaque or 2 transparent pixels.
        let s0 = *s_ptr.add(i);
        let s1 = *s_ptr.add(i + 1);
        let a0 = s0 >> 24;
        let a1 = s1 >> 24;
        if a0 == 255 && a1 == 255 {
            *d_ptr.add(i) = s0;
            *d_ptr.add(i + 1) = s1;
            i += 2;
            continue;
        }
        if a0 == 0 && a1 == 0 {
            i += 2;
            continue;
        }

        // Load 2 pixels (64 bits) and unpack u8 -> u16.
        let sv = _mm_loadl_epi64(s_ptr.add(i) as *const __m128i);
        let dv = _mm_loadl_epi64(d_ptr.add(i) as *const __m128i);
        let s16 = _mm_unpacklo_epi8(sv, zero);
        let d16 = _mm_unpacklo_epi8(dv, zero);

        // Broadcast alpha channel of each pixel across its 4 u16 lanes.
        let a_lo = _mm_shufflelo_epi16(s16, 0xFF);
        let a_vec = _mm_shufflehi_epi16(a_lo, 0xFF);
        let inv_a = _mm_sub_epi16(ff, a_vec);

        // (src * a + dst * (255 - a) + 128) >> 8
        let s_mul = _mm_mullo_epi16(s16, a_vec);
        let d_mul = _mm_mullo_epi16(d16, inv_a);
        let sum = _mm_add_epi16(_mm_add_epi16(s_mul, d_mul), round_bias);
        let out16 = _mm_srli_epi16(sum, 8);
        let out8 = _mm_or_si128(_mm_packus_epi16(out16, zero), opaque_alpha);

        _mm_storel_epi64(d_ptr.add(i) as *mut __m128i, out8);
        i += 2;
    }

    if i < len {
        alpha_blend_row_scalar(&mut dst[i..], &src[i..]);
    }
}
