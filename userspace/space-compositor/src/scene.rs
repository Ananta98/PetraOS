//! Cache-friendly contiguous scene graph & window manager state.

use space_core::ipc::{ShmRing, ShmSurface};
use space_core::Rect;
use std::os::raw::c_int;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Floating,
    Tiled,
}

pub struct SurfaceNode {
    pub id: u32,
    pub _pid: i32,
    pub sock_fd: c_int,
    pub title: String,
    pub rect: Rect,
    pub saved_rect: Rect,
    pub workspace: u8,
    pub minimized: bool,
    pub maximized: bool,
    pub focused: bool,
    pub surface: ShmSurface,
    pub c2s_ring: ShmRing,
    pub s2c_ring: ShmRing,
}

/// Contiguous storage for surfaces and their Z-order indices.
pub struct SceneGraph {
    pub nodes: Vec<SurfaceNode>,
    pub z_order: Vec<u32>,
    pub active_workspace: u8,
    pub layout_mode: LayoutMode,
    pub next_id: u32,
}

impl SceneGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::with_capacity(16),
            z_order: Vec::with_capacity(16),
            active_workspace: 0,
            layout_mode: LayoutMode::Floating,
            next_id: 1,
        }
    }

    pub fn add_node(&mut self, mut node: SurfaceNode) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        node.id = id;
        node.workspace = self.active_workspace;
        self.nodes.push(node);
        self.z_order.push(id);
        self.focus_window(id);
        id
    }

    pub fn remove_window(&mut self, id: u32) -> Option<SurfaceNode> {
        self.z_order.retain(|&z| z != id);
        if let Some(pos) = self.nodes.iter().position(|n| n.id == id) {
            let removed = self.nodes.remove(pos);
            if removed.focused {
                if let Some(&top_id) = self.z_order.iter().rev().find(|&&zid| {
                    self.find(zid)
                        .map(|n| n.workspace == self.active_workspace && !n.minimized)
                        .unwrap_or(false)
                }) {
                    self.focus_window(top_id);
                }
            }
            return Some(removed);
        }
        None
    }

    #[inline]
    pub fn find(&self, id: u32) -> Option<&SurfaceNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    #[inline]
    pub fn find_mut(&mut self, id: u32) -> Option<&mut SurfaceNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn focus_window(&mut self, id: u32) {
        for n in &mut self.nodes {
            n.focused = n.id == id;
        }
        self.z_order.retain(|&z| z != id);
        self.z_order.push(id);
    }

    pub fn focused_id(&self) -> Option<u32> {
        self.nodes
            .iter()
            .find(|n| n.focused && n.workspace == self.active_workspace && !n.minimized)
            .map(|n| n.id)
    }

    /// Applies master-stack tiling on the active workspace when `LayoutMode::Tiled` is active.
    pub fn retile(&mut self, work_area: Rect) {
        if self.layout_mode != LayoutMode::Tiled {
            return;
        }
        let ws = self.active_workspace;
        let ids: Vec<u32> = self
            .z_order
            .iter()
            .copied()
            .filter(|&id| {
                self.find(id)
                    .map(|n| n.workspace == ws && !n.minimized)
                    .unwrap_or(false)
            })
            .collect();
        let count = ids.len();
        if count == 0 {
            return;
        }

        let gap = 8;
        let top_pad = 30;
        if count == 1 {
            if let Some(n) = self.find_mut(ids[0]) {
                n.rect = Rect::new(
                    work_area.x + gap,
                    work_area.y + top_pad,
                    (work_area.w - gap * 2).max(200),
                    (work_area.h - top_pad - gap).max(160),
                );
            }
            return;
        }

        let master_w = (work_area.w * 55) / 100 - gap;
        let stack_w = work_area.w - master_w - gap * 3;
        let stack_count = (count - 1) as i32;
        let stack_h = (work_area.h - gap) / stack_count - top_pad;

        if let Some(m) = self.find_mut(ids[0]) {
            m.rect = Rect::new(
                work_area.x + gap,
                work_area.y + top_pad,
                master_w.max(200),
                (work_area.h - top_pad - gap).max(160),
            );
        }

        for (i, &id) in ids[1..].iter().enumerate() {
            let y = work_area.y + top_pad + (i as i32) * (stack_h + top_pad + gap);
            if let Some(n) = self.find_mut(id) {
                n.rect = Rect::new(
                    work_area.x + master_w + gap * 2,
                    y,
                    stack_w.max(200),
                    stack_h.max(120),
                );
            }
        }
    }
}
