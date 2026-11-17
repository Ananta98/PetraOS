//! Vector SVG icon renderer for PetraSpace GUI using `resvg` + `tiny-skia`.
//!
//! Icons are loaded from `/usr/share/icons/space/*.svg` (with compile-time
//! fallback bytes from the rootfs tree) and cached per `(IconId, size)`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::geometry::Rect;
use crate::pixel::{Canvas, Color};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconId {
    PetraLogo,
    AppManager,
    Files,
    Terminal,
    Tasks,
    Notepad,
    Calc,
    ControlPanel,
    MyComputer,
    LogOff,
    TurnOff,
}

impl IconId {
    /// Infers an icon from a window title or binary name.
    pub fn from_title(title: &str) -> Self {
        let lower = title.to_ascii_lowercase();
        if lower.contains("application") || lower.contains("package") || lower.contains("software") {
            Self::AppManager
        } else if lower.contains("file") || lower.contains("document") {
            Self::Files
        } else if lower.contains("term") || lower.contains("cmd") || lower.contains("prompt") {
            Self::Terminal
        } else if lower.contains("task") {
            Self::Tasks
        } else if lower.contains("pad") || lower.contains("note") {
            Self::Notepad
        } else if lower.contains("calc") {
            Self::Calc
        } else if lower.contains("control") || lower.contains("setting") {
            Self::ControlPanel
        } else {
            Self::MyComputer
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::PetraLogo => "petra-logo.svg",
            Self::AppManager => "app-manager.svg",
            Self::Files => "files.svg",
            Self::Terminal => "terminal.svg",
            Self::Tasks => "tasks.svg",
            Self::Notepad => "notepad.svg",
            Self::Calc => "calc.svg",
            Self::ControlPanel => "control-panel.svg",
            Self::MyComputer => "my-computer.svg",
            Self::LogOff => "log-off.svg",
            Self::TurnOff => "turn-off.svg",
        }
    }

    fn embedded_svg(self) -> &'static [u8] {
        match self {
            Self::PetraLogo => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/petra-logo.svg"),
            Self::AppManager => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/app-manager.svg"),
            Self::Files => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/files.svg"),
            Self::Terminal => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/terminal.svg"),
            Self::Tasks => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/tasks.svg"),
            Self::Notepad => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/notepad.svg"),
            Self::Calc => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/calc.svg"),
            Self::ControlPanel => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/control-panel.svg"),
            Self::MyComputer => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/my-computer.svg"),
            Self::LogOff => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/log-off.svg"),
            Self::TurnOff => include_bytes!("../../../base-files/rootfs/usr/share/icons/space/turn-off.svg"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowGlyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

static ICON_CACHE: OnceLock<Mutex<HashMap<(IconId, i32), Vec<u32>>>> = OnceLock::new();

fn load_svg_bytes(id: IconId) -> Vec<u8> {
    let fname = id.filename();
    let sys_path = format!("/usr/share/icons/space/{}", fname);
    if let Ok(bytes) = std::fs::read(&sys_path) {
        return bytes;
    }
    let repo_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../base-files/rootfs/usr/share/icons/space")
        .join(fname);
    if let Ok(bytes) = std::fs::read(repo_path) {
        return bytes;
    }
    id.embedded_svg().to_vec()
}

fn rasterize_svg(id: IconId, target: i32) -> Vec<u32> {
    let svg_bytes = load_svg_bytes(id);
    let opt = resvg::usvg::Options::default();
    if let Ok(tree) = resvg::usvg::Tree::from_data(&svg_bytes, &opt) {
        if let Some(mut pixmap) = tiny_skia::Pixmap::new(target as u32, target as u32) {
            let sz = tree.size();
            let sx = (target as f32) / sz.width().max(1.0);
            let sy = (target as f32) / sz.height().max(1.0);
            let scale = sx.min(sy);
            let tx = ((target as f32) - sz.width() * scale) * 0.5;
            let ty = ((target as f32) - sz.height() * scale) * 0.5;
            let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(tx, ty);
            resvg::render(&tree, transform, &mut pixmap.as_mut());

            return pixmap
                .data()
                .chunks_exact(4)
                .map(|px| {
                    let a = px[3];
                    if a == 0 {
                        0
                    } else if a == 255 {
                        Color::argb(255, px[0], px[1], px[2]).0
                    } else {
                        // Convert tiny-skia premultiplied RGBA to straight ARGB8888
                        let r = ((px[0] as u32 * 255) / (a as u32)).min(255) as u8;
                        let g = ((px[1] as u32 * 255) / (a as u32)).min(255) as u8;
                        let b = ((px[2] as u32 * 255) / (a as u32)).min(255) as u8;
                        Color::argb(a, r, g, b).0
                    }
                })
                .collect();
        }
    }
    vec![0u32; (target * target) as usize]
}

/// Draws a cached vector SVG icon scaled to `size x size`, clipped to `canvas.bounds()`.
pub fn draw_icon(canvas: &mut dyn Canvas, x: i32, y: i32, size: i32, id: IconId) {
    let target = size.clamp(8, 128);
    let cache = ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    let pixels = guard
        .entry((id, target))
        .or_insert_with(|| rasterize_svg(id, target));
    let clip = canvas.bounds();
    canvas.blit_from(
        pixels,
        target as u32,
        Rect::new(0, 0, target, target),
        x,
        y,
        clip,
        true,
    );
}

/// Draws crisp Windows XP window control glyphs (`_`, `□`, `×`) centered inside `rect`.
pub fn draw_window_glyph(canvas: &mut dyn Canvas, rect: Rect, glyph: WindowGlyph, color: Color) {
    let cx = rect.x + rect.w / 2;
    let cy = rect.y + rect.h / 2;
    match glyph {
        WindowGlyph::Minimize => {
            canvas.fill_rect(Rect::new(cx - 4, cy + 3, 7, 2), color);
        }
        WindowGlyph::Maximize => {
            canvas.fill_rect(Rect::new(cx - 4, cy - 4, 9, 2), color);
            canvas.draw_rect_outline(Rect::new(cx - 4, cy - 4, 9, 9), 1, color);
        }
        WindowGlyph::Restore => {
            canvas.draw_rect_outline(Rect::new(cx - 2, cy - 5, 7, 7), 1, color);
            canvas.draw_rect_outline(Rect::new(cx - 5, cy - 2, 7, 7), 1, color);
        }
        WindowGlyph::Close => {
            for i in -4..=4i32 {
                canvas.fill_rect(Rect::new(cx + i, cy + i, 2, 2), color);
                canvas.fill_rect(Rect::new(cx + i, cy - i, 2, 2), color);
            }
        }
    }
}
