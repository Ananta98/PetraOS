// SPDX-License-Identifier: GPL-2.0

//! 100% Safe xHCI Host Controller Initialization and Command Management.
//!
//! Directly interacts with MMIO registers through [`XhciRegisters`] and [`ostd::io::IoMem`],
//! eliminating raw pointers and `unsafe` code entirely.

use ostd::{
    io::IoMem,
    mm::{HasPaddr, dma::DmaCoherent, io::util::HasVmReaderWriter},
};
use spin::Mutex;

use super::{
    regs::{
        PORTSC_CCS, PORTSC_PED, PORTSC_PP, PORTSC_PR, PORTSC_RW1C_MASK, PORTSC_SPEED_MASK,
        PORTSC_SPEED_SHIFT, USBCMD_HC_RESET, USBCMD_INTE, USBCMD_RUN_STOP, USBSTS_CNR,
        USBSTS_HC_HALTED, XhciRegisters,
    },
    ring::{CommandRing, EventRing, TRB_TYPE_COMMAND_COMPLETION, TRB_TYPE_TRANSFER_EVENT, Trb},
};
use crate::uapi::errno::{Errno, Result};

pub const MAX_SLOTS: usize = 64;

/// High-level xHCI Host Controller instance.
pub struct XhciController {
    pub mmio: IoMem,
    pub regs: XhciRegisters,
    pub dcbaa: DmaCoherent,
    pub dcbaa_paddr: u64,
    pub cmd_ring: Mutex<CommandRing>,
    pub event_ring: Mutex<EventRing>,
    pub num_ports: u8,
    pub is_csz_64: bool,
}

impl XhciController {
    /// Creates and initializes an xHCI controller with acquired MMIO region.
    pub fn new(mmio: IoMem, _phys_base: usize) -> Result<Self> {
        let regs = XhciRegisters::new(&mmio)?;

        // 1. Halt controller if running
        let mut usbcmd = regs.read_usbcmd(&mmio)?;
        usbcmd &= !USBCMD_RUN_STOP;
        regs.write_usbcmd(&mmio, usbcmd)?;

        let mut timeout = 100_000;
        while timeout > 0 {
            if (regs.read_usbsts(&mmio)? & USBSTS_HC_HALTED) != 0 {
                break;
            }
            core::hint::spin_loop();
            timeout -= 1;
        }

        // 2. Reset host controller
        let mut usbcmd = regs.read_usbcmd(&mmio)?;
        usbcmd |= USBCMD_HC_RESET;
        regs.write_usbcmd(&mmio, usbcmd)?;

        timeout = 100_000;
        while timeout > 0 {
            let cmd = regs.read_usbcmd(&mmio)?;
            let sts = regs.read_usbsts(&mmio)?;
            if (cmd & USBCMD_HC_RESET) == 0 && (sts & USBSTS_CNR) == 0 {
                break;
            }
            core::hint::spin_loop();
            timeout -= 1;
        }

        // 3. Query controller parameters
        let num_ports = regs.num_ports;
        let max_device_slots = regs.max_device_slots;
        let is_csz_64 = regs.is_csz_64;

        ostd::info!(
            "xHCI: Controller running with {} ports, {} slots, context size: {}-byte",
            num_ports,
            max_device_slots,
            if is_csz_64 { 64 } else { 32 }
        );

        // 4. Set enabled device slots in CONFIG register
        regs.write_config(&mmio, max_device_slots.min(MAX_SLOTS as u8))?;

        // 5. Allocate Device Context Base Address Array (DCBAA)
        // DCBAA size: (MaxSlotsEn + 1) * 8 bytes (each slot context entry is a 64-bit pointer)
        let dcbaa = DmaCoherent::alloc(1, true).map_err(|_| Errno::ENOMEM)?;
        let dcbaa_paddr = dcbaa.paddr() as u64;

        // Zero DCBAA
        let mut writer = dcbaa.writer();
        for _ in 0..(MAX_SLOTS + 1) {
            let _ = writer.write_val(&0u64);
        }

        // Write DCBAAP
        regs.write_dcbaap(&mmio, dcbaa_paddr)?;

        // 6. Allocate Command Ring and program CRCR
        let cmd_ring = CommandRing::new()?;
        let cmd_paddr = cmd_ring.paddr();
        regs.write_crcr(&mmio, cmd_paddr, true)?;

        // 7. Allocate Event Ring and program Interrupter 0
        let event_ring = EventRing::new()?;
        let erst_paddr = event_ring.erst_paddr;
        let erdp = event_ring.current_dequeue_ptr();

        regs.setup_interrupter_0(&mmio, 1, erst_paddr, erdp)?;

        // 8. Start Controller by setting Run/Stop and INTE bits in USBCMD
        let start_cmd = USBCMD_RUN_STOP | USBCMD_INTE;
        regs.write_usbcmd(&mmio, start_cmd)?;

        timeout = 100_000;
        while timeout > 0 {
            if (regs.read_usbsts(&mmio)? & USBSTS_HC_HALTED) == 0 {
                break;
            }
            core::hint::spin_loop();
            timeout -= 1;
        }

        ostd::info!("xHCI: Host Controller successfully running");

        Ok(Self {
            mmio,
            regs,
            dcbaa,
            dcbaa_paddr,
            cmd_ring: Mutex::new(cmd_ring),
            event_ring: Mutex::new(event_ring),
            num_ports,
            is_csz_64,
        })
    }

    /// Rings the doorbell for the specified slot and endpoint/target.
    pub fn ring_doorbell(&self, slot_id: u8, target: u8) {
        let _ = self.regs.ring_doorbell(&self.mmio, slot_id, target);
    }

    /// Submits a command TRB to the Command Ring, rings doorbell 0,
    /// and waits for Command Completion Event.
    pub fn send_command(&self, parameter: u64, status: u32, control: u32) -> Result<Trb> {
        let expected_trb_ptr = {
            let mut cmd = self.cmd_ring.lock();
            cmd.enqueue(parameter, status, control)?
        };

        // Ring Doorbell 0 (Host Controller Command Ring)
        self.ring_doorbell(0, 0);

        // Poll Event Ring for Command Completion Event
        let mut timeout = 200_000;
        while timeout > 0 {
            let mut ev_ring = self.event_ring.lock();
            if let Some(event) = ev_ring.poll_event()? {
                let erdp = ev_ring.current_dequeue_ptr();
                let _ = self.regs.update_erdp(&self.mmio, erdp);

                if event.trb_type() == TRB_TYPE_COMMAND_COMPLETION {
                    if event.parameter == expected_trb_ptr {
                        let cc = event.completion_code();
                        if cc != 1 {
                            ostd::warn!("xHCI: Command completed with error code {}", cc);
                            return Err(Errno::EIO);
                        }
                        return Ok(event);
                    }
                }
            }
            core::hint::spin_loop();
            timeout -= 1;
        }

        crate::return_errno!(ETIMEDOUT, "xHCI: command completion timeout")
    }

    /// Waits for a Transfer Event matching the specified slot ID and TRB physical address.
    pub fn wait_transfer(&self, slot_id: u8, expected_trb_ptr: u64) -> Result<Trb> {
        let mut timeout = 200_000;
        while timeout > 0 {
            let mut ev_ring = self.event_ring.lock();
            if let Some(event) = ev_ring.poll_event()? {
                let erdp = ev_ring.current_dequeue_ptr();
                let _ = self.regs.update_erdp(&self.mmio, erdp);

                if event.trb_type() == TRB_TYPE_TRANSFER_EVENT {
                    if event.slot_id() == slot_id && event.parameter == expected_trb_ptr {
                        let cc = event.completion_code();
                        if cc != 1 && cc != 13 {
                            // 1 = Success, 13 = Short Packet (allowed on partial transfers)
                            ostd::warn!("xHCI: Transfer event failed with code {}", cc);
                            return Err(Errno::EIO);
                        }
                        return Ok(event);
                    }
                }
            }
            core::hint::spin_loop();
            timeout -= 1;
        }

        crate::return_errno!(ETIMEDOUT, "xHCI: transfer completion timeout")
    }

    /// Checks if a port has a connected device.
    pub fn is_port_connected(&self, port_idx: usize) -> bool {
        if port_idx >= self.num_ports as usize {
            return false;
        }
        self.regs
            .read_portsc(&self.mmio, port_idx)
            .map(|p| (p & PORTSC_CCS) != 0)
            .unwrap_or(false)
    }

    /// Returns the port connection speed (1=FS, 2=LS, 3=HS, 4=SS).
    pub fn port_speed(&self, port_idx: usize) -> u8 {
        self.regs
            .read_portsc(&self.mmio, port_idx)
            .map(|p| ((p & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT) as u8)
            .unwrap_or(0)
    }

    /// Resets a root hub port and waits for it to become enabled.
    pub fn reset_port(&self, port_idx: usize) -> Result<()> {
        if port_idx >= self.num_ports as usize {
            return Err(Errno::EINVAL);
        }

        let portsc = self.regs.read_portsc(&self.mmio, port_idx)?;
        // Preserve regular bits, clear RW1C bits to 0, assert Port Power and Port Reset
        let new_portsc = (portsc & !PORTSC_RW1C_MASK) | PORTSC_PP | PORTSC_PR;
        self.regs.write_portsc(&self.mmio, port_idx, new_portsc)?;

        let mut timeout = 100_000;
        while timeout > 0 {
            if let Ok(sts) = self.regs.read_portsc(&self.mmio, port_idx) {
                if (sts & PORTSC_PED) != 0 {
                    return Ok(());
                }
            }
            core::hint::spin_loop();
            timeout -= 1;
        }

        crate::return_errno!(EIO, "xHCI: port reset timeout")
    }
}
