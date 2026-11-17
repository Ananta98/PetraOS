//! Dynamic vector SVG icon renderer for PetraSpace GUI using `resvg` + `tiny-skia`.
//!
//! Icons are resolved dynamically by name from `/usr/share/icons/space/<name>.svg`
//! (with workspace fallback for host unit tests) and cached per `(name, size)`.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::geometry::Rect;
use crate::pixel::{Canvas, Color};

/// Dynamic icon identifier backed by an icon name or path (never hardcoded to a fixed enum).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IconId(pub Cow<'static, str>);

impl IconId {
    /// Creates a compile-time constant `IconId` from a static icon name.
    pub const fn new(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// Creates a runtime `IconId` from an owned or dynamic icon name/path.
    pub fn from_name(name: impl Into<String>) -> Self {
        Self(Cow::Owned(name.into()))
    }

    /// Returns the icon name or path string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Resolves an icon dynamically from a window title or binary name.
    pub fn from_title(title: &str) -> Self {
        let lower = title.to_ascii_lowercase();
        let slug: String = lower
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let trimmed = slug.trim_matches('-');
        if !trimmed.is_empty() && find_icon_file(trimmed).is_some() {
            return Self::from_name(trimmed.to_string());
        }

        if lower.contains("application") || lower.contains("package") || lower.contains("software") {
            Self::new("app-manager")
        } else if lower.contains("file") || lower.contains("document") || lower.contains("explorer") {
            Self::new("files")
        } else if lower.contains("term") || lower.contains("cmd") || lower.contains("prompt") {
            Self::new("terminal")
        } else if lower.contains("task") || lower.contains("monitor") {
            Self::new("tasks")
        } else if lower.contains("pad") || lower.contains("note") || lower.contains("editor") {
            Self::new("notepad")
        } else if lower.contains("calc") {
            Self::new("calc")
        } else if lower.contains("control") || lower.contains("setting") || lower.contains("properties") {
            Self::new("control-panel")
        } else {
            Self::new("my-computer")
        }
    }

    /// Discovers all available icon names in the active icon theme directory.
    pub fn list_available() -> Vec<Self> {
        let mut out = Vec::new();
        for dir in icon_search_dirs() {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for ent in rd.flatten() {
                    let path = ent.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("svg") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            let id = Self::from_name(stem.to_string());
                            if !out.contains(&id) {
                                out.push(id);
                            }
                        }
                    }
                }
                if !out.is_empty() {
                    break;
                }
            }
        }
        out.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        out
    }
}

impl From<&'static str> for IconId {
    fn from(s: &'static str) -> Self {
        Self::new(s)
    }
}

impl From<String> for IconId {
    fn from(s: String) -> Self {
        Self::from_name(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowGlyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

static ICON_CACHE: OnceLock<Mutex<HashMap<(String, i32), Vec<u32>>>> = OnceLock::new();

fn icon_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(4);
    if let Ok(custom) = std::env::var("SPACE_ICON_DIR") {
        if !custom.is_empty() {
            dirs.push(PathBuf::from(custom));
        }
    }
    dirs.push(PathBuf::from("/usr/share/icons/space"));
    dirs.push(PathBuf::from("/usr/share/icons"));
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../base-files/rootfs/usr/share/icons/space"));
    dirs
}

fn find_icon_file(name: &str) -> Option<PathBuf> {
    if name.starts_with('/') {
        let p = PathBuf::from(name);
        if p.is_file() {
            return Some(p);
        }
    }
    let stem = name.strip_suffix(".svg").unwrap_or(name);
    let fname = format!("{}.svg", stem);
    for dir in icon_search_dirs() {
        let candidate = dir.join(&fname);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn load_svg_bytes(id: &IconId) -> Vec<u8> {
    if let Some(path) = find_icon_file(id.as_str()) {
        if let Ok(bytes) = std::fs::read(&path) {
            return bytes;
        }
    }
    // Generic fallback vector icon so unknown runtime icon names still render cleanly.
    br##"<svg width="48" height="48" viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg">
      <rect x="6" y="6" width="36" height="36" rx="5" fill="#d8e3f2" stroke="#235fa8" stroke-width="2.5"/>
      <rect x="12" y="14" width="24" height="6" rx="2" fill="#235fa8"/>
      <rect x="12" y="24" width="18" height="4" rx="1.5" fill="#566170"/>
      <rect x="12" y="31" width="12" height="4" rx="1.5" fill="#566170"/>
    </svg>"##
        .to_vec()
}

fn rasterize_svg(id: &IconId, target: i32) -> Vec<u32> {
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
pub fn draw_icon(canvas: &mut dyn Canvas, x: i32, y: i32, size: i32, id: &IconId) {
    let target = size.clamp(8, 128);
    let cache = ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    let key = (id.as_str().to_string(), target);
    let pixels = guard
        .entry(key)
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

/// Draws crisp window control glyphs (`_`, `□`, `⧉`, `×`) centered inside `rect`.
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
