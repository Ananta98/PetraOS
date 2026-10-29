// SPDX-License-Identifier: GPL-2.0

//! PS/2 Mouse Driver using `ps2-mouse`.
//!
//! Provides PS/2 mouse packet decoding via the `ps2-mouse` crate, relative motion tracking,
//! button state handling, ISA IRQ 12 interrupt processing, and DevFS character device
//! integration (`/dev/mouse`).

use alloc::{string::String, sync::Arc};
use core::sync::atomic::{AtomicU8, Ordering};
use ostd::{
    arch::{
        irq::{IRQ_CHIP, MappedIrqLine},
        trap::TrapFrame,
    },
    irq::IrqLine,
};
pub use ps2_mouse::{Mouse, MouseFlags, MouseState};
use spin::{Mutex, Once};

use super::controller::{I8042_CONTROLLER, I8042Controller, StatusFlags};
use crate::{
    drivers::{
        CharacterDevice,
        input::{self, InputDevice, keymap},
    },
    uapi::{
        errno::Result,
        input::{InputId, bus, key, rel},
    },
    utils::ring_buffer::RingBuffer,
};

const ISA_MOUSE_IRQ: u8 = 12;

/// Mapped hardware interrupt line for mouse.
static MOUSE_IRQ_LINE: Once<MappedIrqLine> = Once::new();

/// Global lock-free ring buffer storing raw mouse packets for `/dev/mouse`.
static MOUSE_BUFFER: RingBuffer<2048> = RingBuffer::new();

/// Global mouse driver instance from ps2-mouse.
static MOUSE: Mutex<Mouse> = Mutex::new(Mouse::new());

/// The input device this mouse reports to (`/dev/input/eventN`).
static INPUT_DEVICE: Once<Arc<InputDevice>> = Once::new();

/// Buttons held down at the previous report, as `EV_KEY` bit flags.
static BUTTON_STATE: AtomicU8 = AtomicU8::new(0);

/// `BTN_LEFT` bit in [`BUTTON_STATE`].
const BUTTON_LEFT: u8 = 1 << 0;
/// `BTN_RIGHT` bit in [`BUTTON_STATE`].
const BUTTON_RIGHT: u8 = 1 << 1;
/// `BTN_MIDDLE` bit in [`BUTTON_STATE`].
const BUTTON_MIDDLE: u8 = 1 << 2;

/// Collector for 3-byte PS/2 mouse packets to feed `/dev/mouse`.
static PACKET_COLLECTOR: Mutex<PacketCollector> = Mutex::new(PacketCollector::new());

// ----------------------------------------------------------------------------
// Packet Collector for /dev/mouse
// ----------------------------------------------------------------------------

struct PacketCollector {
    raw: [u8; 3],
    idx: usize,
}

impl PacketCollector {
    const fn new() -> Self {
        Self {
            raw: [0; 3],
            idx: 0,
        }
    }

    /// Feeds a byte and returns true when a complete 3-byte packet has been assembled.
    fn feed(&mut self, byte: u8) -> bool {
        // First byte must have bit 3 set to 1. If not, discard to resynchronize stream.
        if self.idx == 0 && (byte & 0x08) == 0 {
            return false;
        }

        self.raw[self.idx] = byte;
        self.idx += 1;

        if self.idx >= 3 {
            self.idx = 0;
            true
        } else {
            false
        }
    }

    fn raw_packet(&self) -> &[u8; 3] {
        &self.raw
    }
}

// ----------------------------------------------------------------------------
// Interrupt Handler & Mouse Input Processing
// ----------------------------------------------------------------------------

fn handle_mouse_irq(_frame: &TrapFrame) {
    if let Some(ctrl_lock) = I8042_CONTROLLER.get() {
        let ctrl = ctrl_lock.lock();
        let status = ctrl.read_status();

        // Verify output buffer is full and data is for mouse (AUX buffer full)
        if status.contains(StatusFlags::OUTPUT_FULL) && status.contains(StatusFlags::AUX_FULL) {
            let byte = ctrl.read_data();
            drop(ctrl); // release controller lock

            let mut mouse = MOUSE.lock();
            mouse.process_packet(byte);

            let mut collector = PACKET_COLLECTOR.lock();
            if collector.feed(byte) {
                for &b in collector.raw_packet() {
                    MOUSE_BUFFER.push_overwrite(b);
                }
                // The status byte carries the middle-button bit; `ps2-mouse`
                // exposes only left/right through `MouseState`.
                let middle = collector.raw_packet()[0] & 0x04 != 0;
                if let Some(device) = INPUT_DEVICE.get() {
                    report_motion(device, &mouse.get_state(), middle);
                }
            }
        }
    }
}

/// Emits one motion/button report into the input core.
///
/// PS/2 reports Y positive-up; evdev (`REL_Y`) is positive-down, hence the
/// negation. Only changed buttons produce `EV_KEY`, and an idle packet produces
/// no report at all.
fn report_motion(device: &InputDevice, state: &MouseState, middle: bool) {
    let mut buttons = 0u8;
    if state.left_button_down() {
        buttons |= BUTTON_LEFT;
    }
    if state.right_button_down() {
        buttons |= BUTTON_RIGHT;
    }
    if middle {
        buttons |= BUTTON_MIDDLE;
    }
    let previous = BUTTON_STATE.swap(buttons, Ordering::Relaxed);

    let mut reported = false;
    for (bit, code) in [
        (BUTTON_LEFT, key::BTN_LEFT),
        (BUTTON_RIGHT, key::BTN_RIGHT),
        (BUTTON_MIDDLE, key::BTN_MIDDLE),
    ] {
        let was = previous & bit != 0;
        let is = buttons & bit != 0;
        if was != is {
            device.report_key(code, is as i32);
            reported = true;
        }
    }

    let dx = state.get_x() as i32;
    let dy = -(state.get_y() as i32);
    if dx != 0 {
        device.report_rel(rel::X, dx);
        reported = true;
    }
    if dy != 0 {
        device.report_rel(rel::Y, dy);
        reported = true;
    }

    if reported {
        device.sync();
    }
}

// ----------------------------------------------------------------------------
// Unified Device Model Trait Implementation
// ----------------------------------------------------------------------------

pub struct MouseDevice;

impl CharacterDevice for MouseDevice {
    fn name(&self) -> String {
        "mouse".into()
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut count = 0;
        while count < buf.len() {
            if let Some(byte) = MOUSE_BUFFER.pop() {
                buf[count] = byte;
                count += 1;
            } else {
                break;
            }
        }
        Ok(count)
    }

    fn write(&self, buf: &[u8]) -> Result<usize> {
        Ok(buf.len())
    }
}

// ----------------------------------------------------------------------------
// Initialization & Detection
// ----------------------------------------------------------------------------

/// Probes and initializes the PS/2 mouse, binds ISA IRQ 12,
/// and registers the driver.
pub fn init(controller: &I8042Controller) -> Result<()> {
    ostd::info!("PS/2 Mouse: Initializing (ps2-mouse)...");

    // Initialize mouse using ps2-mouse crate
    let mut mouse = MOUSE.lock();
    if let Err(err) = mouse.init() {
        ostd::warn!("PS/2 Mouse: ps2-mouse init returned: {:?}", err);
    } else {
        ostd::info!("PS/2 Mouse: ps2-mouse initialized successfully");
    }
    drop(mouse);
    controller.flush_buffer();

    // Register the evdev device (`/dev/input/eventN`) before interrupts can
    // deliver the first packet.
    let device = input::register_device(
        "PS/2 Generic Mouse",
        "isa0060/serio1/input0",
        InputId {
            bustype: bus::I8042,
            vendor: 0x0002,
            product: 0x0001,
            version: 0x0000,
        },
        keymap::mouse_capabilities(&[key::BTN_LEFT, key::BTN_RIGHT, key::BTN_MIDDLE], false),
    );
    INPUT_DEVICE.call_once(|| device);

    // Allocate and map ISA IRQ 12
    if let Some(chip) = IRQ_CHIP.get() {
        if let Ok(irq_line) = IrqLine::alloc() {
            match chip.map_isa_pin_to(irq_line, ISA_MOUSE_IRQ) {
                Ok(mut mapped_line) => {
                    mapped_line.on_active(handle_mouse_irq);
                    MOUSE_IRQ_LINE.call_once(|| mapped_line);
                    ostd::info!("PS/2 Mouse: Mapped ISA IRQ {} successfully", ISA_MOUSE_IRQ);
                }
                Err(err) => {
                    ostd::warn!(
                        "PS/2 Mouse: Failed to map ISA IRQ {}: {:?}",
                        ISA_MOUSE_IRQ,
                        err
                    );
                }
            }
        }
    }

    // Register into unified driver registry
    crate::drivers::register_chrdev(Arc::new(MouseDevice))?;

    ostd::info!("PS/2 Mouse: Driver registered successfully.");
    Ok(())
}
