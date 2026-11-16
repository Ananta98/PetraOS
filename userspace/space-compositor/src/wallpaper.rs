//! Procedural Windows XP "Bliss"-inspired high-resolution wallpaper generator.
//!
//! Renders a deep-azure sky gradient, multi-octave FBM cumulus clouds with
//! sunlit highlights, a distant atmospheric ridge, and rolling emerald hills.

use space_core::Color;

#[inline]
fn hash2(ix: i32, iy: i32) -> f32 {
    let mut n = (ix.wrapping_mul(374_761_393) ^ iy.wrapping_mul(668_265_263)) as u32;
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    ((n ^ (n >> 16)) & 0xFFFF) as f32 / 65535.0
}

#[inline]
fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn value_noise(x: f32, y: f32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = smoothstep(x - ix as f32);
    let fy = smoothstep(y - iy as f32);

    let v00 = hash2(ix, iy);
    let v10 = hash2(ix + 1, iy);
    let v01 = hash2(ix, iy + 1);
    let v11 = hash2(ix + 1, iy + 1);

    let a = v00 + (v10 - v00) * fx;
    let b = v01 + (v11 - v01) * fx;
    a + (b - a) * fy
}

fn fbm_clouds(x: f32, y: f32) -> f32 {
    let mut sum = 0.0f32;
    let mut amp = 0.52f32;
    let mut freq = 1.0f32;
    for _ in 0..4 {
        sum += amp * value_noise(x * freq, y * freq);
        freq *= 2.05;
        amp *= 0.48;
    }
    sum
}

#[inline]
fn mix_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Builds a `w x h` ARGB8888 Bliss-inspired wallpaper buffer.
pub fn build_bliss_wallpaper(w: u32, h: u32) -> Vec<u32> {
    let wi = w.max(1) as usize;
    let hi = h.max(1) as usize;
    let mut buf = vec![0xFF28_66C2; wi * hi];

    let wf = wi as f32;
    let hf = hi as f32;

    let sky_zenith = [32.0, 92.0, 198.0];
    let sky_mid = [74.0, 148.0, 228.0];
    let sky_horizon = [166.0, 214.0, 248.0];

    let distant_top = [78.0, 134.0, 122.0];
    let distant_bot = [58.0, 108.0, 96.0];

    let hill_sun = [126.0, 204.0, 56.0];
    let hill_mid = [78.0, 162.0, 42.0];
    let hill_deep = [48.0, 118.0, 28.0];

    for x in 0..wi {
        let u = x as f32 / wf;
        // Distant misty ridge
        let far_y = hf * (0.49
            + 0.032 * (u * 5.2 + 0.6).sin()
            + 0.015 * (u * 11.4 + 2.1).cos());
        // Main rolling Bliss foreground hill
        let near_y = hf * (0.56
            + 0.045 * (u * 4.4 - 0.5).sin()
            + 0.022 * (u * 9.0 + 1.4).cos()
            - 0.035 * (-(u - 0.32) * (u - 0.32) * 14.0).exp());
        // Foreground darker slope
        let fore_y = hf * (0.74
            + 0.030 * (u * 3.8 + 2.2).sin()
            + 0.012 * (u * 8.1).cos());

        for y in 0..hi {
            let v = y as f32 / hf;
            let yf = y as f32;

            let rgb = if yf < far_y {
                // Sky gradient
                let sky_t = (v / 0.56).clamp(0.0, 1.0);
                let sky = if sky_t < 0.55 {
                    mix_rgb(sky_zenith, sky_mid, sky_t / 0.55)
                } else {
                    mix_rgb(sky_mid, sky_horizon, (sky_t - 0.55) / 0.45)
                };

                // Soft cumulus cloud layer in upper-mid sky
                if v > 0.08 && v < 0.52 {
                    let band = 1.0 - ((v - 0.28) / 0.22).abs().clamp(0.0, 1.0);
                    let n = fbm_clouds(u * 6.5 + 1.3, v * 9.5 + 0.7);
                    let density = ((n - 0.43) * 2.8 * band).clamp(0.0, 1.0);
                    if density > 0.01 {
                        // Top of cloud is bright white, underside is soft cool blue-grey
                        let shade_n = fbm_clouds(u * 6.5 + 1.3, v * 9.5 + 0.55);
                        let lit = ((shade_n - n + 0.12) * 3.2).clamp(0.0, 1.0);
                        let cloud_col = mix_rgb([204.0, 222.0, 242.0], [255.0, 255.0, 255.0], lit);
                        mix_rgb(sky, cloud_col, density * 0.92)
                    } else {
                        sky
                    }
                } else {
                    sky
                }
            } else if yf < near_y {
                let t = ((yf - far_y) / (near_y - far_y).max(1.0)).clamp(0.0, 1.0);
                mix_rgb(distant_top, distant_bot, t)
            } else if yf < fore_y {
                let t = ((yf - near_y) / (fore_y - near_y).max(1.0)).clamp(0.0, 1.0);
                let micro = (value_noise(u * 48.0, v * 48.0) - 0.5) * 8.0;
                let base = mix_rgb(hill_sun, hill_mid, t);
                [base[0] + micro, base[1] + micro, base[2] + micro * 0.5]
            } else {
                let t = ((yf - fore_y) / (hf - fore_y).max(1.0)).clamp(0.0, 1.0);
                let micro = (value_noise(u * 48.0, v * 48.0) - 0.5) * 7.0;
                let base = mix_rgb(hill_mid, hill_deep, t);
                [base[0] + micro, base[1] + micro, base[2] + micro * 0.5]
            };

            let r = rgb[0].clamp(0.0, 255.0) as u8;
            let g = rgb[1].clamp(0.0, 255.0) as u8;
            let b = rgb[2].clamp(0.0, 255.0) as u8;
            buf[y * wi + x] = Color::rgb(r, g, b).0;
        }
    }

    buf
}
