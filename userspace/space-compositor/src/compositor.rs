//! Proprietary damage-tracked compositing engine, evdev router, and IPC server.

use crate::decorator::{HitPart, WindowDecorator, XpLunaDecorator, BORDER_W, TITLEBAR_H};
use crate::panel::{DesktopPanel, PanelAction, TASKBAR_H};
use crate::scene::{LayoutMode, SceneGraph, SurfaceNode};
use space_core::font;
use space_core::icons::{self, IconId};
use space_core::ipc::{
    HandshakePacket, IpcTransport, MsgKind, RingMsg, ShmRing, ShmSurface, COMPOSITOR_SOCKET,
};
use space_core::{sys, Canvas, Color, DamageRegion, Point, Rect};
use space_hal::input::{BTN_LEFT, KEY_ESC, KEY_TAB};
use space_hal::{InputBackend, InputEvent, RenderBackend};
use std::ffi::CString;
use std::os::raw::{c_int, c_void};
use std::process::Command;
use std::ptr;
use std::sync::atomic::Ordering;

enum DragState {
    None,
    Move { win_id: u32, offset_x: i32, offset_y: i32 },
    Resize { win_id: u32, start_pos: Point, orig_rect: Rect },
}

struct ScreenCanvas<'a> {
    width: u32,
    height: u32,
    stride: u32,
    clip: Rect,
    pixels: &'a mut [u32],
}

impl<'a> Canvas for ScreenCanvas<'a> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn stride_pixels(&self) -> u32 {
        self.stride
    }
    fn bounds(&self) -> Rect {
        self.clip
    }
    fn pixels_mut(&mut self) -> &mut [u32] {
        self.pixels
    }
    fn pixels(&self) -> &[u32] {
        self.pixels
    }
}

const DESKTOP_SHORTCUTS: [(&str, &str, IconId); 6] = [
    ("My Computer", "space-files", IconId::MyComputer),
    ("My Documents", "space-files", IconId::Files),
    ("Terminal", "space-term", IconId::Terminal),
    ("Notepad", "space-pad", IconId::Notepad),
    ("Calculator", "space-calc", IconId::Calc),
    ("Control Panel", "space-control", IconId::ControlPanel),
];

fn shortcut_rect(idx: usize) -> Rect {
    Rect::new(14, 14 + (idx as i32) * 82, 118, 74)
}

pub struct Compositor<R: RenderBackend, I: InputBackend> {
    renderer: R,
    input: I,
    scene: SceneGraph,
    panel: DesktopPanel,
    decorator: XpLunaDecorator,
    wallpaper: Vec<u32>,
    selected_shortcut: Option<usize>,
    last_shortcut_click: (Option<usize>, u64),
    damage: DamageRegion,
    listen_fd: c_int,
    cursor: Point,
    prev_cursor: Point,
    drag: DragState,
    frame_tick: u64,
    running: bool,
}

impl<R: RenderBackend, I: InputBackend> Compositor<R, I> {
    pub fn new(renderer: R, input: I) -> std::io::Result<Self> {
        let _ = std::fs::create_dir_all("/run");
        if let Ok(c) = CString::new(COMPOSITOR_SOCKET) {
            unsafe { sys::unlink(c.as_ptr()) };
        }
        let listen_fd = unsafe {
            sys::socket(
                sys::AF_UNIX,
                sys::SOCK_STREAM | sys::SOCK_NONBLOCK | sys::SOCK_CLOEXEC,
                0,
            )
        };
        if listen_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let (addr, addr_len) = sys::make_sockaddr_un(COMPOSITOR_SOCKET)?;
        if unsafe { sys::bind(listen_fd, &addr, addr_len) } < 0 {
            let err = std::io::Error::last_os_error();
            unsafe { sys::close(listen_fd) };
            return Err(err);
        }
        if unsafe { sys::listen(listen_fd, 32) } < 0 {
            let err = std::io::Error::last_os_error();
            unsafe { sys::close(listen_fd) };
            return Err(err);
        }

        let w = renderer.width() as i32;
        let h = renderer.height() as i32;
        let mut damage = DamageRegion::new();
        damage.add(Rect::new(0, 0, w, h));

        let mut panel = DesktopPanel::new();
        panel.update_stats();
        let wallpaper = crate::wallpaper::build_bliss_wallpaper(renderer.width(), renderer.height());

        Ok(Self {
            renderer,
            input,
            scene: SceneGraph::new(),
            panel,
            decorator: XpLunaDecorator,
            wallpaper,
            selected_shortcut: None,
            last_shortcut_click: (None, 0),
            damage,
            listen_fd,
            cursor: Point::new(w / 2, h / 2),
            prev_cursor: Point::new(w / 2, h / 2),
            drag: DragState::None,
            frame_tick: 0,
            running: true,
        })
    }

    pub fn spawn_app(&self, bin: &str) {
        let _ = Command::new(bin).spawn();
    }

    pub fn run(&mut self) {
        let mut events = Vec::with_capacity(64);
        while self.running {
            self.accept_clients();
            self.poll_client_rings();

            events.clear();
            let sw = self.renderer.width() as i32;
            let sh = self.renderer.height() as i32;
            self.input.poll_events(sw, sh, &mut events);
            for ev in events.drain(..) {
                self.handle_input(ev);
            }

            if self.frame_tick % 15 == 0 {
                self.poll_display_mode_request();
            }

            if self.frame_tick % 60 == 0 {
                self.panel.update_stats();
                let sw = self.renderer.width() as i32;
                let sh = self.renderer.height() as i32;
                self.damage.add(self.panel.panel_rect(sw, sh));
            }

            if !self.damage.is_empty() {
                self.compose_damaged();
                let _ = self.renderer.present(self.damage.rects());
                self.damage.clear();
            }

            self.frame_tick += 1;
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }

    fn accept_clients(&mut self) {
        loop {
            let client_fd = unsafe { sys::accept(self.listen_fd, ptr::null_mut(), ptr::null_mut()) };
            if client_fd < 0 {
                break;
            }
            let mut pkt = HandshakePacket::new(0, 0, 0, "", "");
            let pkt_bytes = unsafe {
                std::slice::from_raw_parts_mut(
                    &mut pkt as *mut HandshakePacket as *mut u8,
                    std::mem::size_of::<HandshakePacket>(),
                )
            };
            let n = unsafe { sys::read(client_fd, pkt_bytes.as_mut_ptr() as *mut c_void, pkt_bytes.len()) };
            if n as usize != std::mem::size_of::<HandshakePacket>() || pkt.magic != HandshakePacket::MAGIC {
                unsafe { sys::close(client_fd) };
                continue;
            }
            let prefix = pkt.prefix_str();
            let Ok(surface) = ShmSurface::open(&format!("{}_surf", prefix), pkt.width, pkt.height) else {
                unsafe { sys::close(client_fd) };
                continue;
            };
            let Ok(c2s_ring) = ShmRing::open(&format!("{}_c2s", prefix)) else {
                unsafe { sys::close(client_fd) };
                continue;
            };
            let Ok(s2c_ring) = ShmRing::open(&format!("{}_s2c", prefix)) else {
                unsafe { sys::close(client_fd) };
                continue;
            };

            let offset = (self.scene.nodes.len() as i32 % 6) * 32;
            let sw = self.renderer.width() as i32;
            let sh = self.renderer.height() as i32;
            let base_x = ((sw - pkt.width as i32) / 2).max(148) + offset;
            let base_y = ((sh - TASKBAR_H - pkt.height as i32) / 2).max(48) + offset;
            let rect = Rect::new(base_x, base_y, pkt.width as i32, pkt.height as i32);
            let node = SurfaceNode {
                id: 0,
                _pid: pkt.pid,
                sock_fd: client_fd,
                title: pkt.title_str(),
                rect,
                saved_rect: rect,
                workspace: self.scene.active_workspace,
                minimized: false,
                maximized: false,
                focused: true,
                surface,
                c2s_ring,
                s2c_ring,
            };
            let id = self.scene.add_node(node);
            let wa = self.panel.work_area(sw, sh);
            self.scene.retile(wa);
            if let Some(n) = self.scene.find(id) {
                self.damage.add(self.decorator.shadow_rect(n.rect));
            }
            self.damage.add(self.panel.panel_rect(sw, sh));
        }
    }

    fn poll_display_mode_request(&mut self) {
        let Ok(req) = std::fs::read_to_string("/run/space-display-mode") else {
            return;
        };
        let _ = std::fs::remove_file("/run/space-display-mode");
        let Some((ws, hs)) = req.trim().split_once('x') else {
            return;
        };
        let (Ok(w), Ok(h)) = (ws.parse::<u32>(), hs.parse::<u32>()) else {
            return;
        };
        if let Ok(true) = self.renderer.set_mode(w, h) {
            self.wallpaper = crate::wallpaper::build_bliss_wallpaper(w, h);
            let sw = w as i32;
            let sh = h as i32;
            self.cursor.x = self.cursor.x.clamp(0, sw - 1);
            self.cursor.y = self.cursor.y.clamp(0, sh - 1);
            self.prev_cursor = self.cursor;
            self.scene.retile(self.panel.work_area(sw, sh));
            self.damage.clear();
            self.damage.add(Rect::new(0, 0, sw, sh));
        }
    }

    fn poll_client_rings(&mut self) {
        let mut to_close = Vec::new();
        let sw = self.renderer.width() as i32;
        let sh = self.renderer.height() as i32;
        let screen_bounds = Rect::new(0, 0, sw, sh);

        for node in &self.scene.nodes {
            while let Some(msg) = node.c2s_ring.try_pop() {
                if msg.kind == MsgKind::CommitDamage as u32 {
                    let local = Rect::new(msg.a, msg.b, msg.c, msg.d);
                    let screen_dmg = Rect::new(
                        node.rect.x + local.x,
                        node.rect.y + local.y,
                        local.w,
                        local.h,
                    );
                    self.damage.add_clipped(screen_dmg, screen_bounds);
                } else if msg.kind == MsgKind::CloseRequest as u32 {
                    to_close.push(node.id);
                }
            }
        }

        for id in to_close {
            self.close_window(id);
        }
    }

    fn close_window(&mut self, id: u32) {
        if let Some(node) = self.scene.remove_window(id) {
            let _ = node.s2c_ring.try_push(RingMsg::new(MsgKind::CloseEvent, 0, 0, 0, 0));
            unsafe { sys::close(node.sock_fd) };
            self.damage.add(self.decorator.shadow_rect(node.rect));
            let sw = self.renderer.width() as i32;
            let sh = self.renderer.height() as i32;
            self.scene.retile(self.panel.work_area(sw, sh));
            self.damage.add(self.panel.panel_rect(sw, sh));
        }
    }

    fn handle_input(&mut self, ev: InputEvent) {
        let sw = self.renderer.width() as i32;
        let sh = self.renderer.height() as i32;
        match ev {
            InputEvent::PointerMoveRel { dx, dy } => {
                let next_x = (self.cursor.x + dx).clamp(0, sw - 1);
                let next_y = (self.cursor.y + dy).clamp(0, sh - 1);
                if next_x != self.cursor.x || next_y != self.cursor.y {
                    self.prev_cursor = self.cursor;
                    self.cursor.x = next_x;
                    self.cursor.y = next_y;
                    self.on_pointer_moved();
                }
            }
            InputEvent::PointerMoveAbs { x, y } => {
                let next_x = if x >= 0 { x.clamp(0, sw - 1) } else { self.cursor.x };
                let next_y = if y >= 0 { y.clamp(0, sh - 1) } else { self.cursor.y };
                if next_x != self.cursor.x || next_y != self.cursor.y {
                    self.prev_cursor = self.cursor;
                    self.cursor.x = next_x;
                    self.cursor.y = next_y;
                    self.on_pointer_moved();
                }
            }
            InputEvent::PointerButton { button, pressed } => {
                self.on_pointer_button(button, pressed);
            }
            InputEvent::PointerWheel { delta } => {
                if let Some(id) = self.scene.focused_id() {
                    if let Some(n) = self.scene.find(id) {
                        let lx = self.cursor.x - n.rect.x;
                        let ly = self.cursor.y - n.rect.y;
                        let _ = n.s2c_ring.try_push(RingMsg::new(
                            MsgKind::PointerEvent,
                            lx,
                            ly,
                            0,
                            delta,
                        ));
                    }
                }
            }
            InputEvent::Key { code, pressed, ch, mods } => {
                if pressed {
                    // Global hardware shortcuts
                    if mods.super_key {
                        match code {
                            28 => { self.spawn_app("space-term"); return; } // Super+Enter
                            18 => { self.spawn_app("space-files"); return; } // Super+E
                            16 => {
                                if let Some(id) = self.scene.focused_id() {
                                    self.close_window(id);
                                }
                                return;
                            }
                            20 => {
                                self.toggle_layout();
                                return;
                            }
                            2..=5 => {
                                self.switch_workspace((code - 2) as u8);
                                return;
                            }
                            _ => {}
                        }
                    }
                    if mods.alt && code == KEY_TAB {
                        self.cycle_focus();
                        return;
                    }
                    if code == KEY_ESC && self.panel.launcher_open {
                        self.panel.launcher_open = false;
                        self.damage.add(self.panel.launcher_rect(sh));
                        self.damage.add(self.panel.panel_rect(sw, sh));
                        return;
                    }
                }

                if let Some(id) = self.scene.focused_id() {
                    if let Some(n) = self.scene.find(id) {
                        let _ = n.s2c_ring.try_push(RingMsg::new(
                            MsgKind::KeyEvent,
                            code as i32,
                            pressed as i32,
                            ch.map(|c| c as i32).unwrap_or(0),
                            mods.to_bits(),
                        ));
                    }
                }
            }
        }
    }

    fn on_pointer_moved(&mut self) {
        self.damage.add(Rect::new(self.prev_cursor.x, self.prev_cursor.y, 16, 22));
        self.damage.add(Rect::new(self.cursor.x, self.cursor.y, 16, 22));

        let sh = self.renderer.height() as i32;
        match self.drag {
            DragState::Move { win_id, offset_x, offset_y } => {
                if let Some(n) = self.scene.find_mut(win_id) {
                    let old_sh = self.decorator.shadow_rect(n.rect);
                    n.rect.x = self.cursor.x - offset_x;
                    n.rect.y = (self.cursor.y - offset_y).clamp(TITLEBAR_H, (sh - TASKBAR_H - 8).max(TITLEBAR_H));
                    let new_sh = self.decorator.shadow_rect(n.rect);
                    self.damage.add(old_sh);
                    self.damage.add(new_sh);
                }
            }
            DragState::Resize { win_id, start_pos, orig_rect } => {
                if let Some(n) = self.scene.find_mut(win_id) {
                    let old_sh = self.decorator.shadow_rect(n.rect);
                    n.rect.w = (orig_rect.w + (self.cursor.x - start_pos.x)).max(240);
                    n.rect.h = (orig_rect.h + (self.cursor.y - start_pos.y)).max(160);
                    let new_sh = self.decorator.shadow_rect(n.rect);
                    self.damage.add(old_sh);
                    self.damage.add(new_sh);
                }
            }
            DragState::None => {
                if let Some(id) = self.scene.focused_id() {
                    if let Some(n) = self.scene.find(id) {
                        if n.rect.contains(self.cursor) {
                            let _ = n.s2c_ring.try_push(RingMsg::new(
                                MsgKind::PointerEvent,
                                self.cursor.x - n.rect.x,
                                self.cursor.y - n.rect.y,
                                0,
                                0,
                            ));
                        }
                    }
                }
            }
        }
    }

    fn on_pointer_button(&mut self, button: u16, pressed: bool) {
        let sw = self.renderer.width() as i32;
        let sh = self.renderer.height() as i32;

        if !pressed {
            if !matches!(self.drag, DragState::None) {
                self.drag = DragState::None;
                return;
            }
            if let Some(id) = self.scene.focused_id() {
                if let Some(n) = self.scene.find(id) {
                    let btn_idx = (button - BTN_LEFT + 1) as i32;
                    let _ = n.s2c_ring.try_push(RingMsg::new(
                        MsgKind::PointerEvent,
                        self.cursor.x - n.rect.x,
                        self.cursor.y - n.rect.y,
                        0x200 | btn_idx,
                        0,
                    ));
                }
            }
            return;
        }

        // Check taskbar / start menu first
        match self.panel.handle_click(self.cursor, &self.scene, sw, sh) {
            PanelAction::ToggleLauncher => {
                self.panel.launcher_open = !self.panel.launcher_open;
                self.damage.add(self.panel.launcher_rect(sh));
                self.damage.add(self.panel.panel_rect(sw, sh));
                return;
            }
            PanelAction::SwitchWorkspace(ws) => {
                self.switch_workspace(ws);
                return;
            }
            PanelAction::ToggleLayout => {
                self.toggle_layout();
                return;
            }
            PanelAction::FocusWindow(id) => {
                if let Some(n) = self.scene.find_mut(id) {
                    n.minimized = false;
                }
                self.scene.focus_window(id);
                self.damage.add(Rect::new(0, 0, sw, sh));
                return;
            }
            PanelAction::LaunchApp(bin) => {
                self.damage.add(self.panel.launcher_rect(sh));
                self.damage.add(self.panel.panel_rect(sw, sh));
                self.spawn_app(bin);
                return;
            }
            PanelAction::None => {}
        }

        if self.panel.launcher_open {
            self.panel.launcher_open = false;
            self.damage.add(self.panel.launcher_rect(sh));
            self.damage.add(self.panel.panel_rect(sw, sh));
        }

        // Hit-test windows top-to-bottom
        let ws = self.scene.active_workspace;
        let mut hit = None;
        for &id in self.scene.z_order.iter().rev() {
            if let Some(n) = self.scene.find(id) {
                if n.workspace == ws && !n.minimized {
                    let part = self.decorator.hit_test(n.rect, self.cursor);
                    if part != HitPart::None {
                        hit = Some((id, part, n.rect));
                        break;
                    }
                }
            }
        }

        if let Some((id, part, rect)) = hit {
            if let Some(prev) = self.selected_shortcut.take() {
                self.damage.add(shortcut_rect(prev));
            }
            self.scene.focus_window(id);
            self.damage.add(self.decorator.shadow_rect(rect));
            self.damage.add(self.panel.panel_rect(sw, sh));

            match part {
                HitPart::CloseButton => self.close_window(id),
                HitPart::MinimizeButton => {
                    if let Some(n) = self.scene.find_mut(id) {
                        n.minimized = true;
                        self.damage.add(self.decorator.shadow_rect(n.rect));
                        self.damage.add(self.panel.panel_rect(sw, sh));
                    }
                }
                HitPart::MaximizeButton => {
                    let wa = self.panel.work_area(sw, sh);
                    if let Some(n) = self.scene.find_mut(id) {
                        self.damage.add(self.decorator.shadow_rect(n.rect));
                        if n.maximized {
                            n.rect = n.saved_rect;
                            n.maximized = false;
                        } else {
                            n.saved_rect = n.rect;
                            n.rect = Rect::new(
                                wa.x + BORDER_W,
                                wa.y + TITLEBAR_H,
                                wa.w - BORDER_W * 2,
                                wa.h - TITLEBAR_H - BORDER_W,
                            );
                            n.maximized = true;
                        }
                        self.damage.add(self.decorator.shadow_rect(n.rect));
                    }
                }
                HitPart::TitleBar => {
                    self.drag = DragState::Move {
                        win_id: id,
                        offset_x: self.cursor.x - rect.x,
                        offset_y: self.cursor.y - rect.y,
                    };
                }
                HitPart::ResizeCorner => {
                    self.drag = DragState::Resize {
                        win_id: id,
                        start_pos: self.cursor,
                        orig_rect: rect,
                    };
                }
                HitPart::ClientArea => {
                    if let Some(n) = self.scene.find(id) {
                        let btn_idx = (button - BTN_LEFT + 1) as i32;
                        let _ = n.s2c_ring.try_push(RingMsg::new(
                            MsgKind::PointerEvent,
                            self.cursor.x - n.rect.x,
                            self.cursor.y - n.rect.y,
                            0x100 | btn_idx,
                            0,
                        ));
                    }
                }
                HitPart::None => {}
            }
        } else {
            let mut hit_shortcut = None;
            for i in 0..DESKTOP_SHORTCUTS.len() {
                if shortcut_rect(i).contains(self.cursor) {
                    hit_shortcut = Some(i);
                    break;
                }
            }
            if let Some(prev) = self.selected_shortcut {
                self.damage.add(shortcut_rect(prev));
            }
            self.selected_shortcut = hit_shortcut;
            if let Some(idx) = hit_shortcut {
                self.damage.add(shortcut_rect(idx));
                let (last_idx, last_tick) = self.last_shortcut_click;
                if last_idx == Some(idx) && self.frame_tick.saturating_sub(last_tick) <= 55 {
                    self.spawn_app(DESKTOP_SHORTCUTS[idx].1);
                    self.last_shortcut_click = (None, 0);
                } else {
                    self.last_shortcut_click = (Some(idx), self.frame_tick);
                }
            }
        }
    }

    fn switch_workspace(&mut self, ws: u8) {
        if ws < 4 && ws != self.scene.active_workspace {
            self.scene.active_workspace = ws;
            let sw = self.renderer.width() as i32;
            let sh = self.renderer.height() as i32;
            self.scene.retile(self.panel.work_area(sw, sh));
            self.damage.add(Rect::new(0, 0, sw, sh));
        }
    }

    fn toggle_layout(&mut self) {
        self.scene.layout_mode = match self.scene.layout_mode {
            LayoutMode::Floating => LayoutMode::Tiled,
            LayoutMode::Tiled => LayoutMode::Floating,
        };
        let sw = self.renderer.width() as i32;
        let sh = self.renderer.height() as i32;
        self.scene.retile(self.panel.work_area(sw, sh));
        self.damage.add(Rect::new(0, 0, sw, sh));
    }

    fn cycle_focus(&mut self) {
        let ws = self.scene.active_workspace;
        let ids: Vec<u32> = self
            .scene
            .z_order
            .iter()
            .copied()
            .filter(|&id| {
                self.scene
                    .find(id)
                    .map(|n| n.workspace == ws && !n.minimized)
                    .unwrap_or(false)
            })
            .collect();
        if ids.len() >= 2 {
            self.scene.focus_window(ids[0]);
            let sw = self.renderer.width() as i32;
            let sh = self.renderer.height() as i32;
            self.damage.add(Rect::new(0, 0, sw, sh));
        }
    }

    fn compose_damaged(&mut self) {
        let sw = self.renderer.width();
        let sh = self.renderer.height();
        let stride = self.renderer.stride_pixels();
        let rects: Vec<Rect> = self.damage.rects().to_vec();
        let ws = self.scene.active_workspace;
        let z_order = self.scene.z_order.clone();

        let screen_bounds = Rect::new(0, 0, sw as i32, sh as i32);
        let panel_r = self.panel.panel_rect(sw as i32, sh as i32);
        let launcher_r = self.panel.launcher_rect(sh as i32);

        let pixels = self.renderer.back_buffer_mut();
        let mut canvas = ScreenCanvas {
            width: sw,
            height: sh,
            stride,
            clip: screen_bounds,
            pixels,
        };

        for raw_clip in rects {
            let clip = raw_clip.intersect(screen_bounds);
            if clip.is_empty() {
                continue;
            }
            canvas.clip = clip;

            // 1. Bliss-inspired XP wallpaper (cached SIMD blit for zero-seam partial repaints)
            canvas.blit_from(&self.wallpaper, sw, clip, clip.x, clip.y, clip, false);

            // 1b. Clickable desktop shortcut icons on the left
            for (i, &(label, _, icon)) in DESKTOP_SHORTCUTS.iter().enumerate() {
                let sr = shortcut_rect(i);
                if !sr.intersects(clip) {
                    continue;
                }
                let selected = self.selected_shortcut == Some(i);
                let icon_x = sr.x + (sr.w - 36) / 2;
                let icon_y = sr.y + 6;
                icons::draw_icon(&mut canvas, icon_x, icon_y, 36, icon);
                if selected {
                    canvas.fill_rounded_rect(
                        Rect::new(icon_x, icon_y, 36, 36),
                        4,
                        Color::argb(65, 49, 106, 197),
                    );
                }
                let lw = font::measure_text_width(label, 2);
                let lx = (sr.x + (sr.w - lw) / 2).max(4);
                let ly = sr.y + 46;
                if selected {
                    let lbl_r = Rect::new(lx - 4, ly - 1, lw + 8, 18);
                    canvas.fill_rounded_rect(lbl_r, 2, Color::rgb(0x31, 0x6A, 0xC5));
                    font::draw_text(&mut canvas, lx, ly, label, Color::WHITE, 2);
                } else {
                    font::draw_text_shadow(
                        &mut canvas,
                        lx,
                        ly,
                        label,
                        Color::WHITE,
                        Color::argb(215, 0, 0, 0),
                        2,
                        false,
                    );
                }
            }

            // 2. Windows back-to-front
            for &id in &z_order {
                let Some(node) = self.scene.find(id) else {
                    continue;
                };
                if node.workspace != ws || node.minimized {
                    continue;
                }
                let sh_rect = self.decorator.shadow_rect(node.rect);
                if !sh_rect.intersects(clip) {
                    continue;
                }
                self.decorator
                    .paint_frame(&mut canvas, node.rect, &node.title, node.focused, clip);

                let surf_w = node.surface.header().width.load(Ordering::Acquire) as i32;
                let surf_h = node.surface.header().height.load(Ordering::Acquire) as i32;
                let src_rect = Rect::new(0, 0, surf_w.min(node.rect.w), surf_h.min(node.rect.h));
                canvas.blit_from(
                    node.surface.pixels(),
                    surf_w as u32,
                    src_rect,
                    node.rect.x,
                    node.rect.y,
                    clip,
                    false,
                );
            }

            // 3. Bottom taskbar & Start Menu (only when intersected by this damage rect)
            let touches_panel = clip.intersects(panel_r)
                || (self.panel.launcher_open && clip.intersects(launcher_r));
            if touches_panel {
                self.panel.paint(&mut canvas, &self.scene, sw as i32, sh as i32);
            }

            // 4. Crisp Windows XP-style arrow cursor (12x19) with drop shadow
            let cur_r = Rect::new(self.cursor.x, self.cursor.y, 14, 21);
            if cur_r.intersects(clip) {
                const CURSOR_ROWS: [&str; 19] = [
                    "X...........",
                    "XX..........",
                    "XWX.........",
                    "XWWX........",
                    "XWWWX.......",
                    "XWWWWX......",
                    "XWWWWWX.....",
                    "XWWWWWWX....",
                    "XWWWWWWWX...",
                    "XWWWWWWWWX..",
                    "XWWWWWWXXXX.",
                    "XWWWXWWX....",
                    "XWWX.XWWX...",
                    "XWX..XWWX...",
                    "XX....XWWX..",
                    "X.....XWWX..",
                    ".......XX...",
                    "............",
                    "............",
                ];
                let shadow = Color::argb(90, 0, 0, 0);
                for (ry, row) in CURSOR_ROWS.iter().enumerate() {
                    for (rx, ch) in row.bytes().enumerate() {
                        if ch != b'.' {
                            canvas.fill_rect(
                                Rect::new(self.cursor.x + rx as i32 + 1, self.cursor.y + ry as i32 + 2, 1, 1),
                                shadow,
                            );
                        }
                    }
                }
                for (ry, row) in CURSOR_ROWS.iter().enumerate() {
                    for (rx, ch) in row.bytes().enumerate() {
                        let col = match ch {
                            b'X' => Color::BLACK,
                            b'W' => Color::WHITE,
                            _ => continue,
                        };
                        canvas.fill_rect(
                            Rect::new(self.cursor.x + rx as i32, self.cursor.y + ry as i32, 1, 1),
                            col,
                        );
                    }
                }
            }
        }
    }
}
