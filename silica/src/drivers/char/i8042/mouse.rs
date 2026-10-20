// SPDX-License-Identifier: GPL-2.0

//! PS/2 Mouse Driver using `ps2-mouse`.
//!
//! Provides PS/2 mouse packet decoding via the `ps2-mouse` crate, relative motion tracking,
//! button state handling, ISA IRQ 12 interrupt processing, and DevFS character device
//! integration (`/dev/mouse`).

use alloc::{string::String, sync::Arc};
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
use crate::{api::errno::Result, drivers::CharacterDevice, utils::ring_buffer::RingBuffer};

const ISA_MOUSE_IRQ: u8 = 12;

/// Mapped hardware interrupt line for mouse.
static MOUSE_IRQ_LINE: Once<MappedIrqLine> = Once::new();

/// Global lock-free ring buffer storing raw mouse packets for `/dev/mouse`.
static MOUSE_BUFFER: RingBuffer<2048> = RingBuffer::new();

/// Global mouse driver instance from ps2-mouse.
static MOUSE: Mutex<Mouse> = Mutex::new(Mouse::new());

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
            let mut collector = PACKET_COLLECTOR.lock();
            if collector.feed(byte) {
                for &b in collector.raw_packet() {
                    MOUSE_BUFFER.push_overwrite(b);
                }
            }
            mouse.process_packet(byte);
            let state = mouse.get_state();
            if state.left_button_down() || state.right_button_down() || state.moved() {
                ostd::debug!(
                    "[MOUSE] L={}, R={}, dx={}, dy={}",
                    state.left_button_down(),
                    state.right_button_down(),
                    state.get_x(),
                    state.get_y()
                );
            }
        }
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
    crate::drivers::register_chrdev(Arc::new(MouseDevice));

    ostd::info!("PS/2 Mouse: Driver registered successfully.");
    Ok(())
}
