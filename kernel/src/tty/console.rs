//! Framebuffer Console Subsystem (Flanterm Backend)
//!
//! Provides a high-performance, VT100/ANSI-compliant virtual terminal emulator
//! rendering directly to the Limine linear framebuffer using Flanterm.

use core::alloc::Layout;
use core::ffi::c_void;

use crate::device::{CharDevice, Device};
use crate::drivers::char::keyboard::KEY_RING_BUFFER;
use crate::drivers::serial::{PortIoBackend, SerialPort};
use crate::limine::FRAMEBUFFER_REQUEST;
use crate::sync::spinlock::Spinlock;
use crate::tty::termios::{LineDiscipline, WinSize};

// Memory allocation callbacks required by flanterm C library.
unsafe extern "C" fn flanterm_malloc(size: usize) -> *mut c_void {
    if size == 0 {
        return core::ptr::null_mut();
    }
    // SAFETY: We allocate layout with 16-byte alignment to satisfy all flanterm structures.
    let layout = match Layout::from_size_align(size, 16) {
        Ok(l) => l,
        Err(_) => return core::ptr::null_mut(),
    };
    // SAFETY: Allocating raw heap memory in no_std context.
    unsafe { alloc::alloc::alloc(layout) as *mut c_void }
}

unsafe extern "C" fn flanterm_free(ptr: *mut c_void, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    let layout = match Layout::from_size_align(size, 16) {
        Ok(l) => l,
        Err(_) => return,
    };
    // SAFETY: Deallocating previously allocated memory from flanterm_malloc.
    unsafe {
        alloc::alloc::dealloc(ptr as *mut u8, layout);
    }
}

/// Flanterm Terminal Renderer.
pub struct FlantermContext {
    ctx: *mut flanterm::sys::flanterm_context,
}

// SAFETY: All accesses to FlantermContext are guarded by a kernel spinlock.
unsafe impl Send for FlantermContext {}
unsafe impl Sync for FlantermContext {}

impl FlantermContext {
    /// Initialize a new Flanterm context from Limine framebuffer.
    pub fn new() -> Option<Self> {
        let response = FRAMEBUFFER_REQUEST.get_response()?;
        let fb = response.framebuffers().next()?;

        // SAFETY: Initializing flanterm framebuffer context using Limine validated framebuffer info.
        let ctx = unsafe {
            flanterm::sys::flanterm_fb_init(
                Some(flanterm_malloc),
                Some(flanterm_free),
                fb.addr() as *mut u32,
                fb.width() as usize,
                fb.height() as usize,
                fb.pitch() as usize,
                fb.red_mask_size(),
                fb.red_mask_shift(),
                fb.green_mask_size(),
                fb.green_mask_shift(),
                fb.blue_mask_size(),
                fb.blue_mask_shift(),
                core::ptr::null_mut(), // canvas (allocated internally)
                core::ptr::null_mut(), // ansi colours
                core::ptr::null_mut(), // ansi bright colours
                core::ptr::null_mut(), // default bg
                core::ptr::null_mut(), // default fg
                core::ptr::null_mut(), // default bg bright
                core::ptr::null_mut(), // default fg bright
                core::ptr::null_mut(), // font (default built-in font)
                0,                     // font_width
                0,                     // font_height
                0,                     // font_spacing
                0,                     // font_scale_x
                0,                     // font_scale_y
                0,                     // margin
            )
        };

        if ctx.is_null() {
            None
        } else {
            Some(Self { ctx })
        }
    }

    /// Write raw byte slice directly to the flanterm virtual terminal.
    pub fn write_bytes(&mut self, buf: &[u8]) {
        if self.ctx.is_null() || buf.is_empty() {
            return;
        }
        // SAFETY: Calling flanterm_write with validated buffer pointer and length.
        unsafe {
            flanterm::sys::flanterm_write(
                self.ctx,
                buf.as_ptr() as *const core::ffi::c_char,
                buf.len(),
            );
        }
    }

    /// Query the column and row dimensions of the terminal grid.
    pub fn dimensions(&self) -> (u16, u16) {
        if self.ctx.is_null() {
            return (80, 25);
        }
        // SAFETY: Accessing flanterm_context fields within safe boundaries.
        unsafe {
            let term = &*self.ctx;
            (term.cols as u16, term.rows as u16)
        }
    }
}

/// Unified Console manager integrating Flanterm display, Line Discipline, and Serial fallback.
pub struct Console {
    flanterm: Option<FlantermContext>,
    serial: Option<SerialPort<PortIoBackend>>,
    pub ldisc: LineDiscipline,
}

impl Console {
    pub fn new() -> Self {
        let flanterm_opt = FlantermContext::new();
        let winsize = if let Some(ref ft) = flanterm_opt {
            let (cols, rows) = ft.dimensions();
            WinSize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }
        } else {
            WinSize::default()
        };

        let mut serial = SerialPort::new(PortIoBackend::new(0x3F8));
        let serial_opt = if serial.init().is_ok() {
            Some(serial)
        } else {
            None
        };

        Self {
            flanterm: flanterm_opt,
            serial: serial_opt,
            ldisc: LineDiscipline::new(winsize),
        }
    }

    /// Write output bytes through line discipline and render to flanterm + serial.
    pub fn write_output(&mut self, buf: &[u8]) -> usize {
        let processed = self.ldisc.process_output_bytes(buf);
        if let Some(ref mut ft) = self.flanterm {
            ft.write_bytes(&processed);
        }
        if let Some(ref mut ser) = self.serial {
            for &byte in &processed {
                let _ = ser.write_byte(byte);
            }
        }
        buf.len()
    }

    /// Poll keyboard hardware and serial port for new characters and feed into line discipline.
    pub fn poll_input(&mut self) {
        while let Some(byte) = KEY_RING_BUFFER.pop() {
            let echo = self.ldisc.accept_input_byte(byte);
            if !echo.is_empty() {
                if let Some(ref mut ft) = self.flanterm {
                    ft.write_bytes(&echo);
                }
                if let Some(ref mut ser) = self.serial {
                    for &b in &echo {
                        let _ = ser.write_byte(b);
                    }
                }
            }
        }
    }

    /// Read available bytes from the line discipline buffer.
    pub fn read_input(&mut self, buf: &mut [u8]) -> usize {
        self.poll_input();
        self.ldisc.read_bytes(buf)
    }

    /// Check count of available input bytes.
    pub fn available_input(&mut self) -> usize {
        self.poll_input();
        self.ldisc.available_read_bytes()
    }
}

pub static CONSOLE: Spinlock<Option<Console>> = Spinlock::new(None);

/// Global initialize console subsystem.
pub fn init() {
    let console = Console::new();
    let (cols, rows) = (console.ldisc.winsize.ws_col, console.ldisc.winsize.ws_row);
    *CONSOLE.lock() = Some(console);
    log::info!(
        "[Console] Flanterm Framebuffer Console initialized ({}x{} grid)",
        cols,
        rows
    );
}

/// Notify console that new input is available in the keyboard ring buffer.
/// Attempts a non-blocking lock to immediately poll input and render echo to screen.
pub fn on_input_available() {
    if let Some(mut guard) = CONSOLE.try_lock() {
        if let Some(ref mut console) = *guard {
            console.poll_input();
        }
    }
}
