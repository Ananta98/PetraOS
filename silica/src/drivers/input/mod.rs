// SPDX-License-Identifier: GPL-2.0

//! Input Subsystem Core.
//!
//! Mirrors the Linux input layer: drivers do not talk to userspace directly.
//! They register an [`InputDevice`] describing *what* they can report, then
//! push [`InputEvent`]s into it. The core fans each event out to
//!
//! * **evdev clients** — one lock-free queue per open `/dev/input/eventN`
//!   (see [`evdev`]), and
//! * **input listeners** — in-kernel consumers such as the TTY keyboard
//!   handler (see [`crate::drivers::char::tty::kbd`]).
//!
//! Producers run in interrupt context, so everything on the reporting path is
//! allocation-free and lock-free apart from short spinlocked sections.

pub mod evdev;
pub mod keymap;
pub mod mousedev;
pub mod uinput;
pub mod virtio;

use alloc::{
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicUsize, Ordering};

use ostd::sync::{LocalIrqDisabled, SpinLock};
use spin::Mutex;

use crate::uapi::{
    errno::{Errno, Result},
    input::{
        ABS_MAX, INPUT_PROP_MAX, InputAbsInfo, InputEvent, InputId, KEY_MAX, LED_MAX, MSC_MAX,
        REL_MAX, SND_MAX, SW_MAX, bitmap_len, ev,
    },
};

/// Number of bytes in a key-capability bitmap (`KEY_MAX + 1` bits).
pub const KEY_BITMAP_LEN: usize = bitmap_len(KEY_MAX);
/// Number of bytes in an event-type bitmap (`EV_MAX + 1` bits).
pub const EV_BITMAP_LEN: usize = bitmap_len(ev::MAX as usize);
const REL_BITMAP_LEN: usize = bitmap_len(REL_MAX);
const ABS_BITMAP_LEN: usize = bitmap_len(ABS_MAX);
const MSC_BITMAP_LEN: usize = bitmap_len(MSC_MAX);
const LED_BITMAP_LEN: usize = bitmap_len(LED_MAX);
const SND_BITMAP_LEN: usize = bitmap_len(SND_MAX);
const SW_BITMAP_LEN: usize = bitmap_len(SW_MAX);
const PROP_BITMAP_LEN: usize = bitmap_len(INPUT_PROP_MAX);

/// Events buffered per evdev client. 64 events cover several mouse reports of
/// motion without letting one stalled reader pin memory for long.
pub const EVDEV_QUEUE_CAP: usize = 64;

/// The capabilities an input device advertises.
///
/// These back `EVIOCGBIT`: a driver sets what it reports, and userspace decides
/// how to treat the device from it.
#[derive(Clone, Copy)]
pub struct Capabilities {
    ev: [u8; EV_BITMAP_LEN],
    key: [u8; KEY_BITMAP_LEN],
    rel: [u8; REL_BITMAP_LEN],
    abs: [u8; ABS_BITMAP_LEN],
    msc: [u8; MSC_BITMAP_LEN],
    led: [u8; LED_BITMAP_LEN],
    snd: [u8; SND_BITMAP_LEN],
    sw: [u8; SW_BITMAP_LEN],
    prop: [u8; PROP_BITMAP_LEN],
}

impl Capabilities {
    /// Creates an empty capability set (`EV_SYN` is implied and not stored).
    pub const fn new() -> Self {
        Self {
            ev: [0; EV_BITMAP_LEN],
            key: [0; KEY_BITMAP_LEN],
            rel: [0; REL_BITMAP_LEN],
            abs: [0; ABS_BITMAP_LEN],
            msc: [0; MSC_BITMAP_LEN],
            led: [0; LED_BITMAP_LEN],
            snd: [0; SND_BITMAP_LEN],
            sw: [0; SW_BITMAP_LEN],
            prop: [0; PROP_BITMAP_LEN],
        }
    }

    /// Marks an `EV_*` type as reported by this device.
    pub fn set_ev(&mut self, ty: u16) {
        set_bit(&mut self.ev, ty as usize);
    }

    /// Marks a `KEY_*`/`BTN_*` code as reported by this device.
    pub fn set_key(&mut self, code: u16) {
        set_bit(&mut self.key, code as usize);
    }

    /// Marks a `REL_*` axis as reported by this device.
    pub fn set_rel(&mut self, code: u16) {
        set_bit(&mut self.rel, code as usize);
    }

    /// Marks an `ABS_*` axis as reported by this device.
    pub fn set_abs(&mut self, code: u16) {
        set_bit(&mut self.abs, code as usize);
    }

    /// Marks a `MSC_*` code as reported by this device.
    pub fn set_msc(&mut self, code: u16) {
        set_bit(&mut self.msc, code as usize);
    }

    /// Marks a `LED_*` code as reported by this device.
    pub fn set_led(&mut self, code: u16) {
        set_bit(&mut self.led, code as usize);
    }

    /// Marks a `SND_*` code as reported by this device.
    pub fn set_snd(&mut self, code: u16) {
        set_bit(&mut self.snd, code as usize);
    }

    /// Marks a `SW_*` code as reported by this device.
    pub fn set_sw(&mut self, code: u16) {
        set_bit(&mut self.sw, code as usize);
    }

    /// Marks an `INPUT_PROP_*` property as reported by this device.
    pub fn set_prop(&mut self, prop: u16) {
        set_bit(&mut self.prop, prop as usize);
    }

    /// Copies the capability bitmap of `ty` into `out`, truncated to its length.
    ///
    /// `ty == EV_SYN` reports the event-type bitmap, exactly like
    /// `EVIOCGBIT(0, len)`.
    pub fn bitmap(&self, ty: u16, out: &mut [u8]) {
        let src: &[u8] = match ty {
            ev::SYN => &self.ev,
            ev::KEY => &self.key,
            ev::REL => &self.rel,
            ev::ABS => &self.abs,
            ev::MSC => &self.msc,
            ev::LED => &self.led,
            ev::SND => &self.snd,
            ev::SW => &self.sw,
            _ => &[],
        };
        copy_bitmap(src, out);
    }

    /// Copies the property bitmap into `out`.
    pub fn prop_bitmap(&self, out: &mut [u8]) {
        copy_bitmap(&self.prop, out);
    }

    /// Maximum payload length of `EVIOCGBIT(ty, ...)`.
    pub fn bitmap_ty_len(ty: u16) -> usize {
        match ty {
            ev::SYN => EV_BITMAP_LEN,
            ev::KEY => KEY_BITMAP_LEN,
            ev::REL => REL_BITMAP_LEN,
            ev::ABS => ABS_BITMAP_LEN,
            ev::MSC => MSC_BITMAP_LEN,
            ev::LED => LED_BITMAP_LEN,
            ev::SND => SND_BITMAP_LEN,
            ev::SW => SW_BITMAP_LEN,
            _ => 0,
        }
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::new()
    }
}

fn set_bit(bitmap: &mut [u8], bit: usize) {
    if let Some(byte) = bitmap.get_mut(bit / 8) {
        *byte |= 1 << (bit % 8);
    }
}

/// Sets or clears `bit` in a state bitmap (the `EVIOCGKEY`/`EVIOCGLED` view).
fn set_bitmap_bit(bitmap: &mut [u8], bit: u16, set: bool) {
    if let Some(byte) = bitmap.get_mut(bit as usize / 8) {
        let mask = 1 << (bit % 8);
        if set {
            *byte |= mask;
        } else {
            *byte &= !mask;
        }
    }
}

/// Copies `src` into `out`, zero-filling the rest of `out`.
fn copy_bitmap(src: &[u8], out: &mut [u8]) {
    let n = src.len().min(out.len());
    out[..n].copy_from_slice(&src[..n]);
    out[n..].fill(0);
}

/// An in-kernel consumer of input events.
///
/// The TTY keyboard handler is the only implementation today: it turns
/// `EV_KEY` transitions into terminal input. [`InputDevice::report`] calls
/// listeners from producer context, so implementations must not sleep.
pub trait InputListener: Send + Sync {
    /// Called once per event, in report (possibly interrupt) context.
    fn event(&self, device: &InputDevice, event: &InputEvent);
}

/// A registered input device: identity, capabilities and delivery state.
pub struct InputDevice {
    index: usize,
    name: String,
    phys: String,
    uniq: String,
    id: InputId,
    caps: Capabilities,
    prop: [u8; PROP_BITMAP_LEN],
    /// Last reported state of every key/button, for `EVIOCGKEY`.
    key_state: SpinLock<[u8; KEY_BITMAP_LEN], LocalIrqDisabled>,
    /// Last reported LED state, for `EVIOCGLED`.
    led_state: SpinLock<[u8; LED_BITMAP_LEN], LocalIrqDisabled>,
    /// Absolute axis parameters and state, for `EVIOCGABS` / `EVIOCSABS`.
    abs_info: SpinLock<[Option<InputAbsInfo>; ABS_MAX + 1], LocalIrqDisabled>,
    /// Auto-repeat settings: `[delay_ms, period_ms]`.
    rep: SpinLock<[u32; 2], LocalIrqDisabled>,
    /// Client ID holding an exclusive `EVIOCGRAB`.
    grabbed_by: SpinLock<Option<usize>, LocalIrqDisabled>,
    /// Open evdev clients, one per `/dev/input/eventN` file description.
    clients: SpinLock<Vec<Weak<evdev::EvdevClient>>, LocalIrqDisabled>,
    /// Optional hardware feedback callback (LEDs, sounds, repeat).
    feedback: SpinLock<Option<Arc<dyn Fn(u16, u16, i32) + Send + Sync>>, LocalIrqDisabled>,
    /// Optional hardware poller callback (VirtIO event queue reap).
    poller: SpinLock<Option<Arc<dyn Fn() + Send + Sync>>, LocalIrqDisabled>,
}

impl InputDevice {
    /// The minor index (`eventN`).
    pub fn index(&self) -> usize {
        self.index
    }

    /// The userspace-visible node name (`eventN`).
    pub fn node_name(&self) -> String {
        alloc::format!("event{}", self.index)
    }

    /// The device name reported through `EVIOCGNAME`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The physical path reported through `EVIOCGPHYS`.
    pub fn phys(&self) -> &str {
        &self.phys
    }

    /// The unique identifier reported through `EVIOCGUNIQ`.
    pub fn uniq(&self) -> &str {
        &self.uniq
    }

    /// The identification block reported through `EVIOCGID`.
    pub fn id(&self) -> InputId {
        self.id
    }

    /// The capability set reported through `EVIOCGBIT`.
    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// Copies the input device property bitmap (`EVIOCGPROP`).
    pub fn properties(&self, out: &mut [u8]) {
        copy_bitmap(&self.prop, out);
    }

    /// Copies the current key/button state (the `EVIOCGKEY` payload).
    pub fn key_state(&self, out: &mut [u8]) {
        copy_bitmap(&*self.key_state.lock(), out);
    }

    /// Copies the current LED state (the `EVIOCGLED` payload).
    pub fn led_state(&self, out: &mut [u8]) {
        copy_bitmap(&*self.led_state.lock(), out);
    }

    /// Retrieves absolute axis parameters for `axis` (`EVIOCGABS`).
    pub fn absinfo(&self, axis: u16) -> Option<InputAbsInfo> {
        if (axis as usize) <= ABS_MAX {
            self.abs_info.lock()[axis as usize]
        } else {
            None
        }
    }

    /// Updates absolute axis parameters for `axis` (`EVIOCSABS`).
    pub fn set_absinfo(&self, axis: u16, info: InputAbsInfo) {
        if (axis as usize) <= ABS_MAX {
            self.abs_info.lock()[axis as usize] = Some(info);
        }
    }

    /// Returns auto-repeat settings `[delay_ms, period_ms]`.
    pub fn rep(&self) -> [u32; 2] {
        *self.rep.lock()
    }

    /// Updates auto-repeat settings `[delay_ms, period_ms]`.
    pub fn set_rep(&self, rep: [u32; 2]) {
        *self.rep.lock() = rep;
    }

    /// Grabs exclusive access for `client_id` (`EVIOCGRAB`).
    pub fn grab(&self, client_id: usize) -> Result<()> {
        let mut grab = self.grabbed_by.lock();
        if let Some(existing) = *grab {
            if existing != client_id {
                return Err(Errno::EBUSY);
            }
        }
        *grab = Some(client_id);
        Ok(())
    }

    /// Releases exclusive grab held by `client_id`.
    pub fn ungrab(&self, client_id: usize) {
        let mut grab = self.grabbed_by.lock();
        if *grab == Some(client_id) {
            *grab = None;
        }
    }

    /// Sets a feedback handler for status changes (e.g. keyboard LEDs, sound, repeat).
    pub fn set_feedback_handler<F: Fn(u16, u16, i32) + Send + Sync + 'static>(&self, f: F) {
        *self.feedback.lock() = Some(Arc::new(f));
    }

    /// Dispatches a feedback event to the hardware device if a handler is installed.
    pub fn send_feedback(&self, ty: u16, code: u16, value: i32) {
        if let Some(handler) = self.feedback.lock().clone() {
            handler(ty, code, value);
        }
    }

    /// Sets an on-demand hardware polling handler for polled input devices.
    pub fn set_poller<F: Fn() + Send + Sync + 'static>(&self, f: F) {
        *self.poller.lock() = Some(Arc::new(f));
    }

    /// Polls the underlying hardware device if a polling handler is installed.
    pub fn poll_hardware(&self) {
        if let Some(p) = self.poller.lock().clone() {
            p();
        }
    }

    /// Length of the capability bitmap `EVIOCGBIT(ty, ...)` can return.
    pub fn capability_bitmap_len(ty: u16) -> usize {
        Capabilities::bitmap_ty_len(ty)
    }

    fn attach_client(&self, client: &Arc<evdev::EvdevClient>) {
        let mut clients = self.clients.lock();
        clients.push(Arc::downgrade(client));
    }

    fn detach_client(&self, client: &evdev::EvdevClient) {
        let target = client as *const evdev::EvdevClient;
        self.clients
            .lock()
            .retain(|c| !core::ptr::eq(c.as_ptr(), target));
    }

    fn deliver(&self, event: &InputEvent) {
        let grabbed = *self.grabbed_by.lock();
        for client in self.clients.lock().iter() {
            if let Some(client) = client.upgrade() {
                if let Some(grab_id) = grabbed {
                    if client.id() != grab_id {
                        continue;
                    }
                }
                client.push(event);
            }
        }
    }

    /// Reports one event: updates the cached state, sends it to evdev clients
    /// and to every registered listener.
    ///
    /// Safe to call from interrupt context. A driver should report the whole
    /// change and then call [`InputDevice::sync`] once.
    pub fn report(&self, ty: u16, code: u16, value: i32) {
        let event = InputEvent::now(ty, code, value);
        self.report_event(event);
    }

    /// Reports a `EV_KEY` transition (`value` 1 = press, 0 = release).
    pub fn report_key(&self, code: u16, value: i32) {
        self.report(ev::KEY, code, value);
    }

    /// Reports a `EV_REL` motion delta.
    pub fn report_rel(&self, code: u16, value: i32) {
        self.report(ev::REL, code, value);
    }

    /// Reports a `EV_ABS` absolute position or state.
    pub fn report_abs(&self, code: u16, value: i32) {
        if (code as usize) <= ABS_MAX {
            if let Some(info) = self.abs_info.lock()[code as usize].as_mut() {
                info.value = value;
            }
        }
        self.report(ev::ABS, code, value);
    }

    /// Reports a `EV_LED` state change.
    pub fn report_led(&self, code: u16, value: i32) {
        self.report(ev::LED, code, value);
    }

    /// Injects an event whose timestamp the caller already set (uinput).
    ///
    /// Everything else goes through the same delivery path as hardware events,
    /// so a virtual device is indistinguishable from a physical one.
    pub fn inject(&self, event: InputEvent) {
        self.report_event(event);
    }

    /// Ends a report with `EV_SYN`/`SYN_REPORT`.
    ///
    /// Linux semantics: an event is not complete until the synchronising event
    /// arrives, so clients that batch reports key off this.
    pub fn sync(&self) {
        self.report(ev::SYN, ev::SYN_REPORT, 0);
    }

    fn report_event(&self, event: InputEvent) {
        // Mirror absolute state so a client that opens later still sees the
        // current key/LED/ABS mask through EVIOCGKEY / EVIOCGLED / EVIOCGABS.
        match event.type_ {
            ev::KEY => set_bitmap_bit(&mut *self.key_state.lock(), event.code, event.value != 0),
            ev::LED => set_bitmap_bit(&mut *self.led_state.lock(), event.code, event.value != 0),
            ev::ABS => {
                if (event.code as usize) <= ABS_MAX {
                    if let Some(info) = self.abs_info.lock()[event.code as usize].as_mut() {
                        info.value = event.value;
                    }
                }
            }
            _ => {}
        }

        self.deliver(&event);

        let mut snapshot: [Option<&'static dyn InputListener>; MAX_LISTENERS] =
            [None; MAX_LISTENERS];
        {
            let listeners = LISTENERS.lock();
            for (slot, listener) in snapshot.iter_mut().zip(listeners.iter()) {
                *slot = Some(*listener);
            }
        }
        for listener in snapshot.iter().flatten() {
            listener.event(self, &event);
        }
    }
}

impl core::fmt::Debug for InputDevice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InputDevice")
            .field("index", &self.index)
            .field("name", &self.name)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Device registry
// ---------------------------------------------------------------------------

/// Registered devices in `eventN` order. Mutated only from thread context.
static DEVICES: Mutex<Vec<Arc<InputDevice>>> = Mutex::new(Vec::new());

/// Next `eventN` minor. Monotonic: a removed device's number is not reused, so
/// an open file description can never observe a different device.
static NEXT_INDEX: AtomicUsize = AtomicUsize::new(0);

static LISTENERS: SpinLock<Vec<&'static dyn InputListener>, LocalIrqDisabled> =
    SpinLock::new(Vec::new());

/// Upper bound on in-kernel listeners; the list is snapshotted into a fixed
/// array so a listener may report back into the device (LED state, ...) without
/// re-entering the listener lock.
const MAX_LISTENERS: usize = 4;

/// Registers a kernel-side consumer of the events of every input device.
pub fn register_listener(listener: &'static dyn InputListener) {
    let mut listeners = LISTENERS.lock();
    if listeners.iter().any(|l| core::ptr::eq(*l, listener)) {
        return;
    }
    listeners.push(listener);
}

/// Registers a new input device and allocates its `eventN` minor.
pub fn register_device(
    name: &str,
    phys: &str,
    id: InputId,
    caps: Capabilities,
) -> Arc<InputDevice> {
    register_device_full(name, phys, "", id, caps, [None; ABS_MAX + 1])
}

/// Registers a new input device with unique ID and absolute axis parameters.
pub fn register_device_full(
    name: &str,
    phys: &str,
    uniq: &str,
    id: InputId,
    caps: Capabilities,
    abs_info: [Option<InputAbsInfo>; ABS_MAX + 1],
) -> Arc<InputDevice> {
    let mut caps = caps;
    // Every device emits SYN_REPORT, so advertise it like Linux does.
    caps.set_ev(ev::SYN);

    let mut prop = [0u8; PROP_BITMAP_LEN];
    caps.prop_bitmap(&mut prop);

    let device = Arc::new(InputDevice {
        index: NEXT_INDEX.fetch_add(1, Ordering::Relaxed),
        name: name.to_string(),
        phys: phys.to_string(),
        uniq: uniq.to_string(),
        id,
        caps,
        prop,
        key_state: SpinLock::new([0; KEY_BITMAP_LEN]),
        led_state: SpinLock::new([0; LED_BITMAP_LEN]),
        abs_info: SpinLock::new(abs_info),
        rep: SpinLock::new([250, 33]), // 250ms delay, 33ms repeat period
        grabbed_by: SpinLock::new(None),
        clients: SpinLock::new(Vec::new()),
        feedback: SpinLock::new(None),
        poller: SpinLock::new(None),
    });

    DEVICES.lock().push(Arc::clone(&device));
    ostd::info!(
        "input: registered {} ({}, caps ev={:#x})",
        device.node_name(),
        device.name(),
        caps.ev[0]
    );
    device
}

/// Unregisters `device`, detaching it from `/dev/input`.
///
/// Open evdev clients keep their queues but stop receiving events, matching
/// Linux' behaviour on device removal.
pub fn unregister_device(device: &Arc<InputDevice>) {
    let mut devices = DEVICES.lock();
    devices.retain(|d| !Arc::ptr_eq(d, device));
}

/// Looks up a device by its `eventN` minor.
pub fn get_device(index: usize) -> Option<Arc<InputDevice>> {
    DEVICES.lock().iter().find(|d| d.index == index).cloned()
}

/// Snapshot of the registered devices, for `/dev/input` enumeration.
pub fn devices() -> Vec<Arc<InputDevice>> {
    DEVICES.lock().clone()
}

/// Registers `client` with `device` so it starts receiving events.
pub fn attach_client(device: &Arc<InputDevice>, client: &Arc<evdev::EvdevClient>) {
    device.attach_client(client);
}

/// Initializes the input subsystem core and helper listeners.
pub fn init() -> Result<()> {
    mousedev::init();
    Ok(())
}

crate::module!(
    "Input Subsystem Core",
    "Ananta98",
    crate::modules::InitcallLevel::Subsys,
    init
);

