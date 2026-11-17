//! Zero-copy SpaceClient connecting to `space-compositor` via `/run/space-compositor.sock`,
//! `mmap`'d `ShmSurface`, and lock-free `ShmRing`s.

use crate::theme::Theme;
use crate::traits::{Lifecycle, Paintable, UiEvent};
use space_core::ipc::{
    HandshakePacket, IpcTransport, MsgKind, RingMsg, ShmRing, ShmSurface, COMPOSITOR_SOCKET,
};
use space_core::{sys, Canvas, Point, Rect};
use std::io;
use std::os::raw::{c_int, c_void};
use std::sync::atomic::Ordering;

pub trait AppLogic: Paintable + Lifecycle {
    fn handle_event(&mut self, event: UiEvent) -> bool;
}

/// Client-side window surface wrapper.
pub struct SurfaceCanvas<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a mut [u32],
}

impl<'a> Canvas for SurfaceCanvas<'a> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn stride_pixels(&self) -> u32 {
        self.width
    }
    fn pixels_mut(&mut self) -> &mut [u32] {
        self.pixels
    }
    fn pixels(&self) -> &[u32] {
        self.pixels
    }
}

pub struct SpaceClient {
    sock_fd: c_int,
    surface: ShmSurface,
    c2s_ring: ShmRing,
    s2c_ring: ShmRing,
    pub width: u32,
    pub height: u32,
    pub theme: Theme,
    running: bool,
    dirty: bool,
}

impl SpaceClient {
    pub fn connect(title: &str, width: u32, height: u32) -> io::Result<Self> {
        let pid = unsafe { sys::getpid() };
        let prefix = format!("space_{}_{}", pid, title.replace(' ', "_"));
        let surf_name = format!("{}_surf", prefix);
        let c2s_name = format!("{}_c2s", prefix);
        let s2c_name = format!("{}_s2c", prefix);

        let surface = ShmSurface::create(&surf_name, width, height).map_err(|e| {
            eprintln!("SpaceClient({}): ShmSurface::create({}) failed: {}", title, surf_name, e);
            e
        })?;
        let c2s_ring = ShmRing::create(&c2s_name).map_err(|e| {
            eprintln!("SpaceClient({}): ShmRing::create({}) failed: {}", title, c2s_name, e);
            e
        })?;
        let s2c_ring = ShmRing::create(&s2c_name).map_err(|e| {
            eprintln!("SpaceClient({}): ShmRing::create({}) failed: {}", title, s2c_name, e);
            e
        })?;

        let sock_fd = unsafe { sys::socket(sys::AF_UNIX, sys::SOCK_STREAM | sys::SOCK_CLOEXEC, 0) };
        if sock_fd < 0 {
            let err = io::Error::last_os_error();
            eprintln!("SpaceClient({}): socket() failed: {}", title, err);
            return Err(err);
        }
        let (addr, addr_len) = sys::make_sockaddr_un(COMPOSITOR_SOCKET)?;
        if unsafe { sys::connect(sock_fd, &addr, addr_len) } < 0 {
            let err = io::Error::last_os_error();
            eprintln!("SpaceClient({}): connect({}) failed: {}", title, COMPOSITOR_SOCKET, err);
            unsafe { sys::close(sock_fd) };
            return Err(err);
        }

        let pkt = HandshakePacket::new(pid, width, height, title, &prefix);
        let pkt_bytes = unsafe {
            std::slice::from_raw_parts(
                &pkt as *const HandshakePacket as *const u8,
                std::mem::size_of::<HandshakePacket>(),
            )
        };
        let written = unsafe {
            sys::write(sock_fd, pkt_bytes.as_ptr() as *const c_void, pkt_bytes.len())
        };
        if written < 0 {
            let err = io::Error::last_os_error();
            unsafe { sys::close(sock_fd) };
            return Err(err);
        }

        Ok(Self {
            sock_fd,
            surface,
            c2s_ring,
            s2c_ring,
            width,
            height,
            theme: Theme::PETRA_SLATE,
            running: true,
            dirty: true,
        })
    }

    pub fn request_close(&mut self) {
        self.running = false;
        let _ = self
            .c2s_ring
            .try_push(RingMsg::new(MsgKind::CloseRequest, 0, 0, 0, 0));
    }

    pub fn run<A: AppLogic>(&mut self, app: &mut A) {
        app.on_init();
        while self.running {
            let mut pending_resize: Option<(u32, u32)> = None;
            while let Some(msg) = self.s2c_ring.try_pop() {
                if msg.kind == MsgKind::CloseEvent as u32 {
                    self.running = false;
                    break;
                } else if msg.kind == MsgKind::PointerEvent as u32 {
                    let pos = Point::new(msg.a, msg.b);
                    let buttons = msg.c;
                    let wheel = msg.d;
                    if wheel != 0 {
                        self.dirty |= app.handle_event(UiEvent::Wheel { pos, delta: wheel });
                    } else if buttons > 0 && (buttons & 0x100) != 0 {
                        self.dirty |= app.handle_event(UiEvent::MouseDown {
                            pos,
                            button: buttons & 0xFF,
                        });
                    } else if buttons > 0 && (buttons & 0x200) != 0 {
                        self.dirty |= app.handle_event(UiEvent::MouseUp {
                            pos,
                            button: buttons & 0xFF,
                        });
                    } else {
                        self.dirty |= app.handle_event(UiEvent::MouseMove { pos, buttons });
                    }
                } else if msg.kind == MsgKind::KeyEvent as u32 {
                    let code = msg.a as u16;
                    let pressed = msg.b != 0;
                    let ch = if msg.c > 0 {
                        char::from_u32(msg.c as u32)
                    } else {
                        None
                    };
                    let mods = msg.d;
                    self.dirty |= app.handle_event(UiEvent::Key {
                        code,
                        pressed,
                        ch,
                        mods,
                    });
                } else if msg.kind == MsgKind::FocusEvent as u32 {
                    self.dirty |= app.handle_event(UiEvent::Focus(msg.a != 0));
                } else if msg.kind == MsgKind::ResizeEvent as u32 {
                    pending_resize = Some((msg.a.max(1) as u32, msg.b.max(1) as u32));
                }
            }

            if !self.running {
                break;
            }

            if let Some((rw, rh)) = pending_resize {
                if rw != self.width || rh != self.height {
                    if self.surface.resize(rw, rh).is_ok() {
                        self.width = rw;
                        self.height = rh;
                    }
                }
                self.dirty |= app.handle_event(UiEvent::Resize {
                    width: self.width,
                    height: self.height,
                });
                self.dirty = true;
            }

            self.dirty |= app.on_tick();

            if self.dirty {
                let w = self.surface.header().width.load(Ordering::Acquire);
                let h = self.surface.header().height.load(Ordering::Acquire);
                {
                    let px = self.surface.pixels_mut();
                    let mut canvas = SurfaceCanvas {
                        width: w,
                        height: h,
                        pixels: px,
                    };
                    canvas.fill_rect(Rect::new(0, 0, w as i32, h as i32), Theme::PETRA_SLATE.bg_window);
                    app.paint(&mut canvas);
                }
                let full = Rect::new(0, 0, w as i32, h as i32);
                self.surface.mark_damaged(full);
                let _ = self.c2s_ring.try_push(RingMsg::new(
                    MsgKind::CommitDamage,
                    0,
                    0,
                    w as i32,
                    h as i32,
                ));
                self.dirty = false;
            }

            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        app.on_close();
    }
}

impl Drop for SpaceClient {
    fn drop(&mut self) {
        unsafe {
            sys::close(self.sock_fd);
        }
    }
}
