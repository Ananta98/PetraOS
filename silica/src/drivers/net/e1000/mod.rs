// SPDX-License-Identifier: GPL-2.0

//! Intel e1000 Gigabit Ethernet Network Driver.

pub mod device;
pub mod hw;
pub mod regs;

use alloc::{format, sync::Arc};
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::io::IoMem;

use crate::{
    api::errno::{Errno, Result},
    drivers::{
        bus::pci::{PciDevice, PciDeviceId, PciDriver, pci_register_driver},
        net::register_network_device,
    },
};
pub use device::E1000Device;

pub struct E1000PciDriver {
    counter: AtomicUsize,
}

impl E1000PciDriver {
    pub const fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
        }
    }
}

static E1000_PCI_IDS: [PciDeviceId; 10] = [
    PciDeviceId::match_device(0x8086, 0x100e), // QEMU 82540EM
    PciDeviceId::match_device(0x8086, 0x1004),
    PciDeviceId::match_device(0x8086, 0x100f),
    PciDeviceId::match_device(0x8086, 0x1010),
    PciDeviceId::match_device(0x8086, 0x1019),
    PciDeviceId::match_device(0x8086, 0x101a),
    PciDeviceId::match_device(0x8086, 0x107c),
    PciDeviceId::match_device(0x8086, 0x10d3), // 82574L
    PciDeviceId::match_device(0x8086, 0x153a),
    PciDeviceId::match_device(0x8086, 0x1502),
];

impl PciDriver for E1000PciDriver {
    fn name(&self) -> &'static str {
        "e1000"
    }

    fn id_table(&self) -> &[PciDeviceId] {
        &E1000_PCI_IDS
    }

    fn probe(&self, dev: &PciDevice) -> Result<()> {
        let index = self.counter.fetch_add(1, Ordering::Relaxed);
        probe_device(dev, index)
    }
}

/// Probes and initializes an Intel e1000 NIC discovered on the PCI bus.
pub fn probe_device(dev: &PciDevice, index: usize) -> Result<()> {
    ostd::info!(
        "e1000: Initializing network controller at PCI {}",
        dev.address
    );

    // 1. Enable Bus Mastering (DMA) and Memory Space
    dev.enable_bus_mastering();
    dev.enable_memory_space();

    // 2. Locate BAR0 (MMIO region)
    let bar0 = dev.bars[0].ok_or_else(|| {
        ostd::error!("e1000: Controller has no valid BAR0");
        Errno::ENODEV
    })?;

    let mmio_range = (bar0.address as usize)..(bar0.address as usize + bar0.size as usize);
    let mmio = IoMem::acquire(mmio_range).map_err(|_| {
        ostd::error!("e1000: Failed to map BAR0 MMIO region");
        Errno::EIO
    })?;

    let name = format!("eth{}", index);
    let e1000 = E1000Device::new(name, mmio)?;

    // 3. Register device in drivers, devfs, and network stack
    register_network_device(e1000);

    Ok(())
}

fn init() -> Result<()> {
    pci_register_driver(Arc::new(E1000PciDriver::new()))
}

crate::module!(
    "Intel e1000 Gigabit Ethernet Driver",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
