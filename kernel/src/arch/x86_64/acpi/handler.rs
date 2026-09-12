//! ACPI Handler Implementation for PetraOS.
//!
//! Implements the `acpi::Handler` trait required by the `acpi` crate to interface
//! with PetraOS physical memory mapping, MMIO, I/O ports, timers, and mutexes.

use crate::arch::cpu::ports::Ports;
use crate::mm::{ensure_mapped, hhdm_offset};
use crate::sync::Mutex;
use acpi::{Handle, Handler, PciAddress, PhysicalMapping};
use alloc::boxed::Box;
use core::ptr::NonNull;

/// Handler implementation for the `acpi` crate.
#[derive(Clone, Debug, Default)]
pub struct PetraAcpiHandler;

impl PetraAcpiHandler {
    pub const fn new() -> Self {
        Self
    }
}

impl Handler for PetraAcpiHandler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        // Ensure memory is mapped in the active page tables.
        ensure_mapped(physical_address as u64, size);

        let hhdm = hhdm_offset();
        let virt_addr = (physical_address as u64 + hhdm) as *mut T;

        PhysicalMapping {
            physical_start: physical_address,
            virtual_start: NonNull::new(virt_addr).expect("virtual address for ACPI table is null"),
            region_length: size,
            mapped_length: size,
            handler: self.clone(),
        }
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {
        // In PetraOS, physical memory remains mapped in the higher-half direct map (HHDM).
    }

    fn read_u8(&self, address: usize) -> u8 {
        ensure_mapped(address as u64, 1);
        let virt = (address as u64 + hhdm_offset()) as *const u8;
        // SAFETY: The physical address is mapped via ensure_mapped.
        unsafe { core::ptr::read_volatile(virt) }
    }

    fn read_u16(&self, address: usize) -> u16 {
        ensure_mapped(address as u64, 2);
        let virt = (address as u64 + hhdm_offset()) as *const u16;
        // SAFETY: The physical address is mapped via ensure_mapped.
        unsafe { core::ptr::read_unaligned(virt) }
    }

    fn read_u32(&self, address: usize) -> u32 {
        ensure_mapped(address as u64, 4);
        let virt = (address as u64 + hhdm_offset()) as *const u32;
        // SAFETY: The physical address is mapped via ensure_mapped.
        unsafe { core::ptr::read_unaligned(virt) }
    }

    fn read_u64(&self, address: usize) -> u64 {
        ensure_mapped(address as u64, 8);
        let virt = (address as u64 + hhdm_offset()) as *const u64;
        // SAFETY: The physical address is mapped via ensure_mapped.
        unsafe { core::ptr::read_unaligned(virt) }
    }

    fn write_u8(&self, address: usize, value: u8) {
        ensure_mapped(address as u64, 1);
        let virt = (address as u64 + hhdm_offset()) as *mut u8;
        // SAFETY: The physical address is mapped via ensure_mapped.
        unsafe { core::ptr::write_volatile(virt, value) }
    }

    fn write_u16(&self, address: usize, value: u16) {
        ensure_mapped(address as u64, 2);
        let virt = (address as u64 + hhdm_offset()) as *mut u16;
        // SAFETY: The physical address is mapped via ensure_mapped.
        unsafe { core::ptr::write_unaligned(virt, value) }
    }

    fn write_u32(&self, address: usize, value: u32) {
        ensure_mapped(address as u64, 4);
        let virt = (address as u64 + hhdm_offset()) as *mut u32;
        // SAFETY: The physical address is mapped via ensure_mapped.
        unsafe { core::ptr::write_unaligned(virt, value) }
    }

    fn write_u64(&self, address: usize, value: u64) {
        ensure_mapped(address as u64, 8);
        let virt = (address as u64 + hhdm_offset()) as *mut u64;
        // SAFETY: The physical address is mapped via ensure_mapped.
        unsafe { core::ptr::write_unaligned(virt, value) }
    }

    fn read_io_u8(&self, port: u16) -> u8 {
        // SAFETY: Direct port I/O read.
        unsafe { Ports::inb(port) }
    }

    fn read_io_u16(&self, port: u16) -> u16 {
        // SAFETY: Direct port I/O read.
        unsafe { Ports::inw(port) }
    }

    fn read_io_u32(&self, port: u16) -> u32 {
        // SAFETY: Direct port I/O read.
        unsafe { Ports::inl(port) }
    }

    fn write_io_u8(&self, port: u16, value: u8) {
        // SAFETY: Direct port I/O write.
        unsafe { Ports::outb(port, value) }
    }

    fn write_io_u16(&self, port: u16, value: u16) {
        // SAFETY: Direct port I/O write.
        unsafe { Ports::outw(port, value) }
    }

    fn write_io_u32(&self, port: u16, value: u32) {
        // SAFETY: Direct port I/O write.
        unsafe { Ports::outl(port, value) }
    }

    fn read_pci_u8(&self, address: PciAddress, offset: u16) -> u8 {
        let pci_addr = 0x8000_0000u32
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);
        // SAFETY: Reading PCI configuration space via standard 0xCF8/0xCFC ports.
        unsafe {
            Ports::outl(0xCF8, pci_addr);
            Ports::inb(0xCFC + (offset & 0x3))
        }
    }

    fn read_pci_u16(&self, address: PciAddress, offset: u16) -> u16 {
        let pci_addr = 0x8000_0000u32
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);
        // SAFETY: Reading PCI configuration space via standard 0xCF8/0xCFC ports.
        unsafe {
            Ports::outl(0xCF8, pci_addr);
            Ports::inw(0xCFC + (offset & 0x2))
        }
    }

    fn read_pci_u32(&self, address: PciAddress, offset: u16) -> u32 {
        let pci_addr = 0x8000_0000u32
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);
        // SAFETY: Reading PCI configuration space via standard 0xCF8/0xCFC ports.
        unsafe {
            Ports::outl(0xCF8, pci_addr);
            Ports::inl(0xCFC)
        }
    }

    fn write_pci_u8(&self, address: PciAddress, offset: u16, value: u8) {
        let pci_addr = 0x8000_0000u32
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);
        // SAFETY: Writing PCI configuration space via standard 0xCF8/0xCFC ports.
        unsafe {
            Ports::outl(0xCF8, pci_addr);
            Ports::outb(0xCFC + (offset & 0x3), value);
        }
    }

    fn write_pci_u16(&self, address: PciAddress, offset: u16, value: u16) {
        let pci_addr = 0x8000_0000u32
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);
        // SAFETY: Writing PCI configuration space via standard 0xCF8/0xCFC ports.
        unsafe {
            Ports::outl(0xCF8, pci_addr);
            Ports::outw(0xCFC + (offset & 0x2), value);
        }
    }

    fn write_pci_u32(&self, address: PciAddress, offset: u16, value: u32) {
        let pci_addr = 0x8000_0000u32
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);
        // SAFETY: Writing PCI configuration space via standard 0xCF8/0xCFC ports.
        unsafe {
            Ports::outl(0xCF8, pci_addr);
            Ports::outl(0xCFC, value);
        }
    }

    fn nanos_since_boot(&self) -> u64 {
        crate::clock::elapsed_ns()
    }

    fn stall(&self, microseconds: u64) {
        crate::clock::sleep_ns(microseconds.saturating_mul(1_000));
    }

    fn sleep(&self, milliseconds: u64) {
        crate::clock::sleep_ns(milliseconds.saturating_mul(1_000_000));
    }

    fn create_mutex(&self) -> Handle {
        let mutex = Box::new(Mutex::new(()));
        let ptr = Box::into_raw(mutex) as usize;
        Handle(ptr as u32)
    }

    fn acquire(&self, mutex: Handle, timeout: u16) -> Result<(), acpi::aml::AmlError> {
        // AML mutexes in simple non-preemptive / single-task boot stage can spin
        let ptr = mutex.0 as usize as *const Mutex<()>;
        if ptr.is_null() {
            return Err(acpi::aml::AmlError::MutexAcquireTimeout);
        }
        // SAFETY: Pointer was created via create_mutex Box::into_raw.
        let m = unsafe { &*ptr };
        if timeout == 0 {
            if m.try_lock().is_some() {
                core::mem::forget(m.try_lock());
                Ok(())
            } else {
                Err(acpi::aml::AmlError::MutexAcquireTimeout)
            }
        } else {
            // Spin lock
            core::mem::forget(m.lock());
            Ok(())
        }
    }

    fn release(&self, mutex: Handle) {
        let ptr = mutex.0 as usize as *const Mutex<()>;
        if !ptr.is_null() {
            // SAFETY: Releasing the mutex lock
            unsafe {
                (*ptr).force_unlock();
            }
        }
    }
}
