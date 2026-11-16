pub mod font;
pub mod geometry;
pub mod ipc;
pub mod pixel;
pub mod simd;
pub mod sys;
pub mod void_pkg;

pub use geometry::{DamageRegion, Point, Rect, Size};
pub use pixel::{Canvas, Color, PixelBuffer};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{IpcTransport, MsgKind, RingMsg, ShmRing, ShmSurface};

    #[test]
    fn damage_region_coalesces_overlapping() {
        let mut dmg = DamageRegion::new();
        dmg.add(Rect::new(0, 0, 100, 100));
        dmg.add(Rect::new(10, 0, 100, 100));
        assert_eq!(dmg.rects().len(), 1);
        assert_eq!(dmg.rects()[0], Rect::new(0, 0, 110, 100));
    }

    #[test]
    fn simd_fill_and_blend() {
        let mut row = vec![Color::rgb(100, 100, 100).0; 16];
        simd::fill_u32(&mut row, Color::rgb(10, 20, 30).0);
        assert!(row.iter().all(|&p| p == Color::rgb(10, 20, 30).0));

        let src = vec![Color::argb(128, 255, 0, 0).0; 16];
        simd::alpha_blend_row(&mut row, &src);
        assert!(Color(row[0]).red() > 100);
    }

    #[test]
    fn lock_free_shm_ring_and_surface() {
        let ring_name = format!("test_ring_{}", std::process::id());
        let ring = ShmRing::create(&ring_name).expect("create ring");
        assert!(ring.try_push(RingMsg::new(MsgKind::CommitDamage, 1, 2, 3, 4)));
        let popped = ring.try_pop().expect("pop msg");
        assert_eq!(popped.kind, MsgKind::CommitDamage as u32);
        assert_eq!((popped.a, popped.b, popped.c, popped.d), (1, 2, 3, 4));
        assert!(ring.try_pop().is_none());

        let surf_name = format!("test_surf_{}", std::process::id());
        let mut surf = ShmSurface::create(&surf_name, 64, 64).expect("create surf");
        surf.pixels_mut()[0] = 0xFF11_2233;
        assert_eq!(surf.pixels()[0], 0xFF11_2233);
    }
}
