// SPDX-License-Identifier: GPL-2.0

//! Advanced Host Controller Interface (AHCI) SATA Driver.
//!
//! Provides PCI bus binding, host controller initialization, memory-mapped
//! I/O management, and SATA drive enumeration integrated with `ostd`.

pub mod fis;
pub mod hba;
pub mod port;

use alloc::{format, sync::Arc, vec::Vec};
use ostd::{io::IoMem, mm::VmIoOnce};

use self::{
    hba::*,
    port::{AhciDeviceType, AhciPort},
};
use crate::{
    api::errno::{Errno, Result},
    drivers::{
        BlockDevice,
        block::register_block_device,
        bus::pci::{self, PciDevice, PciDeviceId, PciDriver, pci_register_driver},
    },
};

/// High-level AHCI Controller handle.
pub struct AhciController {
    pub mmio: IoMem,
    pub ports: Vec<Arc<AhciPort>>,
}

/// Initializes an AHCI controller discovered on the PCI bus.
pub fn probe_controller(dev: &PciDevice) -> Result<Arc<AhciController>> {
    ostd::info!("AHCI: Initializing SATA controller at PCI {}", dev.address);

    // 1. Enable Bus Mastering (DMA) and Memory Mapped I/O
    dev.enable_bus_mastering();
    dev.enable_memory_space();

    // 2. Locate BAR5 (ABAR - AHCI Base Address Register)
    let bar5 = dev.bars[5].ok_or_else(|| {
        ostd::error!("AHCI: Controller at {} has no valid BAR5", dev.address);
        Errno::ENODEV
    })?;

    let mmio_range = (bar5.address as usize)..(bar5.address as usize + bar5.size as usize);
    let mmio = IoMem::acquire(mmio_range).map_err(|_| {
        ostd::error!(
            "AHCI: Failed to map BAR5 MMIO region at {:#X}",
            bar5.address
        );
        Errno::EIO
    })?;

    // 3. Enable AHCI Mode (GHC.AE = 1)
    let mut ghc: u32 = mmio.read_once(AHCI_GHC).map_err(|_| Errno::EIO)?;
    if (ghc & GHC_AE) == 0 {
        ghc |= GHC_AE;
        mmio.write_once(AHCI_GHC, &ghc).map_err(|_| Errno::EIO)?;
    }

    // 4. Read Ports Implemented (PI) bitmap
    let pi: u32 = mmio.read_once(AHCI_PI).map_err(|_| Errno::EIO)?;
    let cap: u32 = mmio.read_once(AHCI_CAP).map_err(|_| Errno::EIO)?;
    let vs: u32 = mmio.read_once(AHCI_VS).map_err(|_| Errno::EIO)?;

    let num_cmd_slots = ((cap >> 8) & 0x1F) + 1;
    ostd::info!(
        "AHCI: Version {:x}.{:02x}, PI mask: {:#010x}, command slots: {}",
        vs >> 16,
        vs & 0xFFFF,
        pi,
        num_cmd_slots
    );

    let mut ports = Vec::new();
    let mut disk_idx = 0;

    // 5. Scan implemented ports (0..32)
    for port_no in 0..32 {
        if (pi & (1 << port_no)) == 0 {
            continue;
        }

        let port_base = AHCI_PORTS_BASE + port_no * 0x80;
        let dev_type = AhciPort::probe_device_type(&mmio, port_base)?;

        match dev_type {
            AhciDeviceType::Sata => {
                let name = format!("sd{}", (b'a' + disk_idx) as char);
                disk_idx += 1;

                ostd::info!(
                    "AHCI: Discovered SATA drive on port {}, assigning device name \"{}\"",
                    port_no,
                    name
                );

                match AhciPort::new(port_no, mmio.clone(), name.clone()) {
                    Ok(port) => {
                        // Test write and read to verify AHCI detection and I/O correctness
                        let mut test_write = alloc::vec![0xAAu8; 512];
                        test_write[0..8].copy_from_slice(b"SILICA_S");
                        match port.write_blocks(0, &test_write) {
                            Ok(written) => {
                                ostd::info!("AHCI: Self-test write 512 bytes to LBA 0 ok ({} bytes written)", written);
                                let mut test_read = alloc::vec![0u8; 512];
                                match port.read_blocks(0, &mut test_read) {
                                    Ok(read) => {
                                        if test_read[0..8] == b"SILICA_S"[..] {
                                            ostd::info!("AHCI: Self-test read 512 bytes verified successfully! (magic {:?})", &test_read[0..8]);
                                        } else {
                                            ostd::error!("AHCI: Self-test read data mismatch: {:?}", &test_read[0..16]);
                                        }
                                    }
                                    Err(e) => {
                                        ostd::error!("AHCI: Self-test read failed: {:?}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                ostd::error!("AHCI: Self-test write failed: {:?}", e);
                            }
                        }

                        let port_arc = Arc::new(port);
                        register_block_device(&name, Arc::clone(&port_arc) as _);
                        ports.push(port_arc);
                    }
                    Err(e) => {
                        ostd::error!("AHCI: Failed to initialize port {}: {:?}", port_no, e);
                    }
                }
            }
            AhciDeviceType::Satapi => {
                ostd::info!("AHCI: Port {}: SATAPI optical drive detected (skipping)", port_no);
            }
            AhciDeviceType::Semb => {
                ostd::info!("AHCI: Port {}: SEMB enclosure management bridge detected", port_no);
            }
            AhciDeviceType::PortMultiplier => {
                ostd::info!("AHCI: Port {}: Port multiplier detected", port_no);
            }
            _ => {}
        }
    }

    ostd::info!(
        "AHCI: Controller at {} initialized ({} active SATA drive(s))",
        dev.address,
        ports.len()
    );

    Ok(Arc::new(AhciController { mmio, ports }))
}

// ----------------------------------------------------------------------------
// PCI Driver Implementation & Kernel Module Declaration
// ----------------------------------------------------------------------------

pub struct AhciPciDriver;

static AHCI_PCI_IDS: [PciDeviceId; 1] = [PciDeviceId::match_class(
    pci::ids::class::MASS_STORAGE,
    pci::ids::mass_storage::SATA,
    0x01, // AHCI 1.0 programming interface
)];

impl PciDriver for AhciPciDriver {
    fn name(&self) -> &'static str {
        "ahci"
    }

    fn id_table(&self) -> &[PciDeviceId] {
        &AHCI_PCI_IDS
    }

    fn probe(&self, dev: &PciDevice) -> Result<()> {
        probe_controller(dev).map(|_| ())
    }
}

fn init() -> Result<()> {
    pci_register_driver(Arc::new(AhciPciDriver))
}

crate::module!(
    "AHCI SATA Storage Driver",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
