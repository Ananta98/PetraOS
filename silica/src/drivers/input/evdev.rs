// SPDX-License-Identifier: GPL-2.0

//! evdev clients (`/dev/input/eventN`).
//!
//! An evdev client is one open file description on an input device. It owns a
//! ring of events (pushed by the device's interrupt handler), a wait queue for
//! blocking readers and the `EVIOCG*` capability queries. The VFS node that
//! hands these clients out lives in [`crate::fs::devfs::input`].

use alloc::{collections::VecDeque, sync::Arc, vec};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use ostd::sync::{LocalIrqDisabled, SpinLock, WaitQueue, Waiter};

use super::{EVDEV_QUEUE_CAP, InputDevice};
use crate::{
    uapi::{
        errno::{Errno, Result},
        input::{
            ABS_MAX, CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_REALTIME, InputAbsInfo, InputEvent,
            ev, ioctl,
        },
        ioctl as ioc,
    },
    utils::userspace::user_ptr::UserPtr,
};

/// Linux `EV_VERSION`, reported by `EVIOCGVERSION`.
const EV_VERSION: i32 = 0x010001;

/// Monotonic ID generator for evdev clients.
static NEXT_CLIENT_ID: AtomicUsize = AtomicUsize::new(1);

/// One reader of an input device.
///
/// The queue is a short spinlock-protected ring rather than a lock-free
/// structure: producers are interrupt handlers and consumers are threads, and
/// `LocalIrqDisabled` already excludes the producer from the consumer's
/// critical section, so the lock is never contended for long and never
/// deadlocks against an interrupt.
pub struct EvdevClient {
    id: usize,
    device: Arc<InputDevice>,
    /// Events pushed by the producer, popped by this client's reader.
    queue: SpinLock<VecDeque<InputEvent>, LocalIrqDisabled>,
    /// Readers parked on an empty queue.
    readers: WaitQueue,
    /// Indicates an event was dropped due to queue overflow.
    dropped: AtomicBool,
    /// Revoked status (`EVIOCREVOKE`).
    revoked: AtomicBool,
    /// Clock domain for timestamps (`EVIOCSCLOCKID`).
    clock_id: AtomicI32,
}

impl EvdevClient {
    /// Creates a client and attaches it to `device`.
    pub fn new(device: Arc<InputDevice>) -> Arc<Self> {
        let client = Arc::new(Self {
            id: NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed),
            device,
            queue: SpinLock::new(VecDeque::with_capacity(EVDEV_QUEUE_CAP)),
            readers: WaitQueue::new(),
            dropped: AtomicBool::new(false),
            revoked: AtomicBool::new(false),
            clock_id: AtomicI32::new(CLOCK_REALTIME),
        });
        super::attach_client(&client.device, &client);
        client
    }

    /// Client identifier.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Whether this client description has been revoked.
    pub fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
    }

    /// The device this client reads from.
    pub fn device(&self) -> &Arc<InputDevice> {
        &self.device
    }

    /// Bytes a reader can consume without blocking.
    pub fn available(&self) -> usize {
        if self.is_revoked() {
            return 0;
        }
        self.device.poll_hardware();
        let dropped_extra = if self.dropped.load(Ordering::Relaxed) {
            InputEvent::SIZE
        } else {
            0
        };
        self.queue.lock().len() * InputEvent::SIZE + dropped_extra
    }

    /// Buffers one event and wakes a parked reader. Producer context.
    ///
    /// On overflow the oldest event is dropped and `SYN_DROPPED` is queued on next read.
    pub fn push(&self, event: &InputEvent) {
        if self.is_revoked() {
            return;
        }
        {
            let mut queue = self.queue.lock();
            if queue.len() >= EVDEV_QUEUE_CAP {
                queue.pop_front();
                self.dropped.store(true, Ordering::Relaxed);
            }
            queue.push_back(*event);
        }
        self.readers.wake_one();
        crate::fs::poll::notify();
    }

    /// Pops events into `buf`, never splitting one.
    ///
    /// Returns the number of bytes copied, or `0` when the queue is empty.
    pub fn try_read(&self, buf: &mut [u8]) -> Result<usize> {
        if self.is_revoked() {
            return Err(Errno::ENODEV);
        }

        self.device.poll_hardware();

        let capacity = buf.len() / InputEvent::SIZE;
        if capacity == 0 {
            return Err(Errno::EINVAL);
        }

        let mut queue = self.queue.lock();
        let mut copied = 0;

        // If events were dropped, synthesize SYN_DROPPED as the very first event
        if self.dropped.swap(false, Ordering::Relaxed) {
            let dropped_event = InputEvent::now(ev::SYN, ev::SYN_DROPPED, 0);
            buf[0..InputEvent::SIZE].copy_from_slice(&dropped_event.to_bytes());
            copied += 1;
        }

        while copied < capacity {
            let Some(event) = queue.pop_front() else {
                break;
            };
            let start = copied * InputEvent::SIZE;
            buf[start..start + InputEvent::SIZE].copy_from_slice(&event.to_bytes());
            copied += 1;
        }
        Ok(copied * InputEvent::SIZE)
    }

    /// Reads events, sleeping until at least one is available.
    ///
    /// `nonblock` turns an empty queue into `EAGAIN`. A pending signal ends the
    /// sleep with `EINTR`, like any other interruptible read.
    pub fn read(&self, buf: &mut [u8], nonblock: bool) -> Result<usize> {
        if self.is_revoked() {
            return Err(Errno::ENODEV);
        }

        if buf.len() / InputEvent::SIZE == 0 {
            return Err(Errno::EINVAL);
        }

        if nonblock {
            let n = self.try_read(buf)?;
            if n == 0 {
                return Err(Errno::EAGAIN);
            }
            return Ok(n);
        }

        let (waiter, _) = Waiter::new_pair();

        // Publish the waker before the queue is inspected: a producer that
        // pushes between the check and the sleep would otherwise be lost.
        let cond = || -> Option<usize> {
            if self.is_revoked() {
                return None;
            }
            self.readers.enqueue(waiter.waker());
            match self.try_read(buf) {
                Ok(0) | Err(_) => None,
                Ok(n) => Some(n),
            }
        };

        let cancel = || {
            if self.is_revoked() {
                Err(Errno::ENODEV)
            } else if crate::proc::thread::Thread::should_interrupt() {
                Err(Errno::EINTR)
            } else {
                Ok(())
            }
        };

        waiter.wait_until_or_cancelled(cond, cancel)
    }

    /// Writes events from userspace (e.g. setting keyboard LEDs, force feedback, sound).
    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        if self.is_revoked() {
            return Err(Errno::ENODEV);
        }
        if buf.len() % InputEvent::SIZE != 0 {
            return Err(Errno::EINVAL);
        }

        for chunk in buf.chunks_exact(InputEvent::SIZE) {
            let type_ = u16::from_ne_bytes([chunk[16], chunk[17]]);
            let code = u16::from_ne_bytes([chunk[18], chunk[19]]);
            let value = i32::from_ne_bytes([chunk[20], chunk[21], chunk[22], chunk[23]]);

            match type_ {
                ev::LED => {
                    self.device.report_led(code, value);
                    self.device.send_feedback(type_, code, value);
                }
                ev::SND => {
                    self.device.report(type_, code, value);
                    self.device.send_feedback(type_, code, value);
                }
                ev::REP => {
                    let mut rep = self.device.rep();
                    if code as usize == crate::uapi::input::rep::DELAY {
                        rep[0] = value as u32;
                    } else if code as usize == crate::uapi::input::rep::PERIOD {
                        rep[1] = value as u32;
                    }
                    self.device.set_rep(rep);
                    self.device.send_feedback(type_, code, value);
                }
                _ => {}
            }
        }
        Ok(buf.len())
    }

    /// Implements the `EVIOCG*`/`FIONREAD` ioctl set.
    ///
    /// Buffer-length requests (`EVIOCGNAME`, `EVIOCGBIT`, ...) carry their size
    /// in the request number and return the number of bytes written.
    pub fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        if self.is_revoked() {
            return Err(Errno::ENODEV);
        }

        let proc = crate::proc::thread::Thread::proc_or_esrch()?;
        let vmar = proc.vmspace();
        let ptr = UserPtr::new(arg);
        let len = ioc::ioc_size(cmd);
        let device = &self.device;

        match cmd {
            ioc::FIONREAD => {
                let pending = self.available() as i32;
                ptr.write_bytes(&vmar, &pending.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::EVIOCGVERSION => {
                ptr.write_bytes(&vmar, &EV_VERSION.to_ne_bytes())?;
                Ok(0)
            }
            ioctl::EVIOCGID => {
                ptr.write_bytes(&vmar, &device.id().to_bytes())?;
                Ok(0)
            }
            ioctl::EVIOCGREP => {
                let rep = device.rep();
                let mut bytes = [0u8; 8];
                bytes[0..4].copy_from_slice(&rep[0].to_ne_bytes());
                bytes[4..8].copy_from_slice(&rep[1].to_ne_bytes());
                ptr.write_bytes(&vmar, &bytes)?;
                Ok(0)
            }
            ioctl::EVIOCSREP => {
                let mut bytes = [0u8; 8];
                ptr.read_bytes(&vmar, &mut bytes)?;
                let delay = u32::from_ne_bytes(bytes[0..4].try_into().unwrap());
                let period = u32::from_ne_bytes(bytes[4..8].try_into().unwrap());
                device.set_rep([delay, period]);
                Ok(0)
            }
            ioctl::EVIOCGRAB => {
                if arg != 0 {
                    device.grab(self.id)?;
                } else {
                    device.ungrab(self.id);
                }
                Ok(0)
            }
            ioctl::EVIOCREVOKE => {
                self.revoked.store(true, Ordering::Release);
                self.readers.wake_all();
                crate::fs::poll::notify();
                Ok(0)
            }
            ioctl::EVIOCSCLOCKID => {
                let clk: i32 = ptr.read_val(&vmar)?;
                if clk != CLOCK_REALTIME && clk != CLOCK_MONOTONIC && clk != CLOCK_BOOTTIME {
                    return Err(Errno::EINVAL);
                }
                self.clock_id.store(clk, Ordering::Relaxed);
                Ok(0)
            }
            _ if cmd == ioctl::eviocgname(len) => copy_string(&ptr, &vmar, device.name(), len),
            _ if cmd == ioctl::eviocgphys(len) => copy_string(&ptr, &vmar, device.phys(), len),
            _ if cmd == ioctl::eviocguniq(len) => copy_string(&ptr, &vmar, device.uniq(), len),
            _ if cmd == ioctl::eviocgprop(len) => {
                let mut bitmap = vec![0u8; len];
                device.properties(&mut bitmap);
                ptr.write_bytes(&vmar, &bitmap)?;
                Ok(len)
            }
            _ if is_abs_get_query(cmd) => {
                let axis = (ioc::ioc_nr(cmd) - 0x40) as u16;
                let Some(info) = device.absinfo(axis) else {
                    return Err(Errno::EINVAL);
                };
                ptr.write_bytes(&vmar, &info.to_bytes())?;
                Ok(0)
            }
            _ if is_abs_set_query(cmd) => {
                let axis = (ioc::ioc_nr(cmd) - 0xc0) as u16;
                let mut buf = [0u8; InputAbsInfo::SIZE];
                ptr.read_bytes(&vmar, &mut buf)?;
                let Some(info) = InputAbsInfo::from_bytes(&buf) else {
                    return Err(Errno::EINVAL);
                };
                device.set_absinfo(axis, info);
                Ok(0)
            }
            _ if is_capability_query(cmd) => {
                let ty = (ioc::ioc_nr(cmd) - 0x20) as u16;
                self.copy_capability(&ptr, &vmar, ty, len)
            }
            _ if cmd == ioctl::eviocgkey(len) => {
                let mut bitmap = vec![0u8; len.min(super::KEY_BITMAP_LEN)];
                device.key_state(&mut bitmap);
                ptr.write_bytes(&vmar, &bitmap)?;
                Ok(bitmap.len())
            }
            _ if cmd == ioctl::eviocgled(len) => {
                let mut bitmap = vec![0u8; len];
                device.led_state(&mut bitmap);
                ptr.write_bytes(&vmar, &bitmap)?;
                Ok(bitmap.len())
            }
            _ if cmd == ioctl::eviocgsnd(len) || cmd == ioctl::eviocgsw(len) => {
                let zeros = vec![0u8; len];
                ptr.write_bytes(&vmar, &zeros)?;
                Ok(len)
            }
            _ => crate::return_errno!(ENOTTY, "unknown evdev ioctl"),
        }
    }

    fn copy_capability(
        &self,
        ptr: &UserPtr,
        vmar: &Arc<crate::vm::vmar::Vmar>,
        ty: u16,
        len: usize,
    ) -> Result<usize> {
        let n = len.min(InputDevice::capability_bitmap_len(ty));
        let mut bitmap = vec![0u8; n];
        self.device.capabilities().bitmap(ty, &mut bitmap);
        ptr.write_bytes(vmar, &bitmap)?;
        Ok(n)
    }
}

impl Drop for EvdevClient {
    fn drop(&mut self) {
        self.device.ungrab(self.id);
        self.device.detach_client(self);
    }
}

/// Whether `cmd` is an `EVIOCGBIT(ty, len)` request.
fn is_capability_query(cmd: u32) -> bool {
    ioc::ioc_type(cmd) == ioctl::EV_TYPE
        && (0x20..=0x20 + ev::MAX as u32).contains(&ioc::ioc_nr(cmd))
}

/// Whether `cmd` is an `EVIOCGABS(abs)` request.
fn is_abs_get_query(cmd: u32) -> bool {
    ioc::ioc_type(cmd) == ioctl::EV_TYPE
        && (0x40..=0x40 + ABS_MAX as u32).contains(&ioc::ioc_nr(cmd))
        && ioc::ioc_dir(cmd) & ioc::IOC_READ != 0
}

/// Whether `cmd` is an `EVIOCSABS(abs)` request.
fn is_abs_set_query(cmd: u32) -> bool {
    ioc::ioc_type(cmd) == ioctl::EV_TYPE
        && (0xc0..=0xc0 + ABS_MAX as u32).contains(&ioc::ioc_nr(cmd))
        && ioc::ioc_dir(cmd) & ioc::IOC_WRITE != 0
}

/// Copies a NUL-terminated string into user memory, truncated to `len`.
fn copy_string(
    ptr: &UserPtr,
    vmar: &Arc<crate::vm::vmar::Vmar>,
    value: &str,
    len: usize,
) -> Result<usize> {
    if len == 0 {
        return Err(Errno::EINVAL);
    }
    let bytes = value.as_bytes();
    let n = len.min(bytes.len() + 1);
    let mut buf = vec![0u8; n];
    let payload = n.min(bytes.len());
    buf[..payload].copy_from_slice(&bytes[..payload]);
    ptr.write_bytes(vmar, &buf)?;
    Ok(n)
}

/// Opens `device` as a new evdev client.
pub fn open(device: Arc<InputDevice>) -> Arc<EvdevClient> {
    EvdevClient::new(device)
}

