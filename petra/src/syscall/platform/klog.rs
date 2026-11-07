// SPDX-License-Identifier: GPL-2.0

//! The kernel message ring and `syslog(2)` (Syscall #103 on x86_64).
//!
//! A logger backend registered in [`init`] appends every ostd log line into
//! a fixed-capacity ring, then forwards the record to the ostd early console
//! so the serial output stays exactly what it was before.
//!
//! `syslog(2)` mirrors Linux's action numbering (`include/linux/syslog.h`):
//! actions 0/1 are no-ops, 2/3/4 read (blocking / peeking / read-and-clear),
//! 5 clears, 6/7/8 control the console level, and 9/10 report buffer sizes.

use alloc::vec::Vec;
use core::fmt::{self, Write};

use ostd::{
    arch::cpu::context::UserContext,
    sync::{LocalIrqDisabled, SpinLock, WaitQueue},
};

use crate::{
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// Ring capacity in bytes (Linux's `CONFIG_LOG_BUF_SHIFT` default is 64 KiB).
pub const KLOG_SIZE: usize = 64 << 10;
/// How many full lines the ring retains (mirrors Linux's retained message count).
const RING_RETAIN: usize = 256;
/// One line is stored this large at most; longer ones stop there.
const LINE_MAX: usize = 256;

/// One retained log line.
struct LineSlot {
    /// Syslog severity, `0`..=`7`, prefixed as `<N>`.
    level: u8,
    /// Message bytes, without the `<N>` prefix or the trailing `\n`.
    text: [u8; LINE_MAX],
    /// Number of meaningful bytes in `text`.
    len: usize,
}

impl LineSlot {
    const fn empty() -> Self {
        Self {
            level: 0,
            text: [0; LINE_MAX],
            len: 0,
        }
    }
}

/// The whole retained-ring state, guarded by a single spin lock.
struct KlogCore {
    /// Retention slots; `lines[head..head+count)` is the oldest-first view.
    lines: [LineSlot; RING_RETAIN],
    head: usize,
    count: usize,
    dropped: u64,
}

static KLOG: SpinLock<KlogCore, LocalIrqDisabled> = SpinLock::new(KlogCore {
    lines: [const { LineSlot::empty() }; RING_RETAIN],
    head: 0,
    count: 0,
    dropped: 0,
});

/// Readers waiting for the ring to gain a line (`SYSLOG_ACTION_READ`).
static RING_WAKE: WaitQueue = WaitQueue::new();

/// The active console log level (`SYSLOG_ACTION_CONSOLE_*`).
static CONSOLE_LEVEL: SpinLock<u8, LocalIrqDisabled> = SpinLock::new(7);

/// A fixed-capacity line formatter; the logger must not allocate.
struct LineBuf {
    text: [u8; LINE_MAX],
    len: usize,
}

impl LineBuf {
    const fn new() -> Self {
        Self {
            text: [0; LINE_MAX],
            len: 0,
        }
    }

    fn push(&mut self, byte: u8) {
        if self.len < LINE_MAX {
            self.text[self.len] = byte;
            self.len += 1;
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.text[..self.len]
    }
}

impl Write for LineBuf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.as_bytes() {
            self.push(*byte);
        }
        Ok(())
    }
}

/// The ostd logger backend: rings the line, then prints it to the console.
struct KernelLogger;

impl ostd::log::Log for KernelLogger {
    fn log(&self, record: &ostd::log::Record) {
        let level = level_number(record.level());

        // Reproduce the exact default console formatting.
        ostd::console::early_print(format_args!(
            "{}: {}{}\n",
            record.level(),
            record.prefix(),
            record.args()
        ));

        let mut line = LineBuf::new();
        let _ = write!(line, "<{}>", level);
        let _ = write!(line, "{}", record.args());
        let _ = line.push(b'\n');
        push_line(level, line.bytes());
        drop(line);
        RING_WAKE.wake_all();
    }
}

/// Computes the syslog numeric level (`0`..=`7`) from an ostd level.
fn level_number(level: ostd::log::Level) -> u8 {
    // ostd's severity values equal the syslog levels one-to-one.
    level as u8
}

/// Appends one formatted line to the ring, dropping the oldest when full.
fn push_line(level: u8, bytes: &[u8]) {
    let mut core = KLOG.lock();
    if core.count == RING_RETAIN {
        core.head = (core.head + 1) % RING_RETAIN;
        core.count -= 1;
        core.dropped += 1;
    }
    let idx = (core.head + core.count) % RING_RETAIN;
    let slot = &mut core.lines[idx];
    slot.level = level;
    let copied = core::cmp::min(bytes.len(), LINE_MAX);
    slot.text[..copied].copy_from_slice(&bytes[..copied]);
    slot.len = copied;
    core.count += 1;
}

/// Console output went through the injected backend, so the ring retains the
/// exact lines a `dmesg` would show.
pub fn init() {
    static LOGGER: KernelLogger = KernelLogger;
    ostd::log::inject_logger(&LOGGER);
}

/// Total unread bytes (`SYSLOG_ACTION_SIZE_UNREAD`).
fn unread_bytes() -> usize {
    let core = KLOG.lock();
    let mut total = 0;
    for i in 0..core.count {
        let idx = (core.head + i) % RING_RETAIN;
        total += core.lines[idx].len + 4;
    }
    total
}

/// Snapshots lines oldest-first into `out` until it fills or the ring ends.
///
/// Returns how many lines were consumed; `consume = true` removes them.
fn snapshot(out: &mut Vec<u8>, consume: bool) -> usize {
    let mut core = KLOG.lock();
    let mut taken = 0;
    for i in 0..core.count {
        let idx = (core.head + i) % RING_RETAIN;
        let slot = &core.lines[idx];
        let mut out_prev = out.len();
        let _ = &mut out_prev;
        let needed = slot.len + 4;
        if out.len() + needed > out.capacity() {
            break;
        }
        out.push(b'<');
        out.push(b'0' + (slot.level % 10));
        out.push(b'>');
        out.extend_from_slice(&slot.text[..slot.len]);
        out.push(b'\n');
        taken += 1;
    }
    if consume && taken > 0 {
        core.head = (core.head + taken) % RING_RETAIN;
        core.count -= taken;
    }
    taken
}

/// The nonblocking read paths: `SYSLOG_ACTION_READ_ALL/CLEAR` (3/4).
fn read_ring(buf_ptr: usize, buf_len: usize, consume: bool) -> Result<usize> {
    if buf_len == 0 || buf_ptr == 0 {
        return Ok(0);
    }
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let mut out = alloc::vec![0u8; core::cmp::min(buf_len, KLOG_SIZE)];
    snapshot(&mut out, consume);
    let written = out.len();
    if written > 0 {
        UserPtr::new(buf_ptr).write_bytes(&vmar, &out)?;
    }
    Ok(written)
}

/// Clears the ring (`SYSLOG_ACTION_CLEAR`).
fn clear() {
    let mut core = KLOG.lock();
    core.head = 0;
    core.count = 0;
}

/// The console level actions: `OFF`(6) / `ON`(7) / `SET LEVEL`(8).
fn console_action(action: i32, new_level: i32) -> Result<usize> {
    if action == 6 && new_level > 0 {
        // Linux lets `CONSOLE_OFF` request a specific suppression level.
        *CONSOLE_LEVEL.lock() = clamp_level(new_level);
        return Ok(0);
    }
    let level = match action {
        6 => 1,
        7 => 7,
        8 => {
            if !(0..=7).contains(&new_level) {
                return Err(Errno::EINVAL);
            }
            *CONSOLE_LEVEL.lock() = new_level as u8;
            return Ok(0);
        }
        _ => return Err(Errno::EINVAL),
    };
    *CONSOLE_LEVEL.lock() = level;
    Ok(0)
}

fn clamp_level(level: i32) -> u8 {
    level.clamp(0, 7) as u8
}

/// System call `syslog(int type, char __user *buf, int len)` (Syscall #103).
pub fn sys_syslog(action: i32, buf_ptr: usize, buf_len: i32, _ctx: &mut UserContext) -> Result<usize> {
    if buf_len < 0 {
        return Err(Errno::EINVAL);
    }
    // Linux gates read/clear/console-control on CAP_SYSLOG.
    let restricted = matches!(action, 2 | 5 | 6 | 7 | 8);
    if restricted && !Thread::proc_or_esrch()?.cred().cr_uid.is_root() {
        return Err(Errno::EPERM);
    }

    match action {
        0 | 1 => Ok(0),
        2 | 3 | 4 => {
            let consume = action == 4;
            if action == 2 {
                // Blocking read: park until a line exists.
                RING_WAKE.wait_until(|| {
                    if unread_bytes() > 0 {
                        Some(())
                    } else if Thread::should_interrupt() {
                        Some(())
                    } else {
                        None
                    }
                });
                if unread_bytes() == 0 {
                    return Err(Errno::EINTR);
                }
            }
            read_ring(buf_ptr, buf_len as usize, consume)
        }
        5 => {
            clear();
            Ok(0)
        }
        6 | 7 | 8 => console_action(action, buf_len),
        9 => Ok(unread_bytes()),
        10 => Ok(KLOG_SIZE),
        _ => Err(Errno::EINVAL),
    }
}
