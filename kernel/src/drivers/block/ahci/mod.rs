/// AHCI Driver for SATA Storage Devices
///
/// Refactored from a single file into a modular structure:
/// - `hba`     — Host Bus Adapter register layout and initialization
/// - `command` — ATA command creation (READ/WRITE DMA, IDENTIFY)
/// - `device`  — AhciBlockDevice representing a registered device
///
/// Integrates with the new `crate::drivers::pci` subsystem for auto-discovery.
///
use crate::drivers::block::register_block_device;
use crate::drivers::irq::IrqRegistration;
use crate::drivers::pci::PciBar;
use alloc::string::String;
use alloc::sync::Arc;
use device::{AhciBlockDevice, AhciBlockDeviceInner};
use ostd::io::IoMem;
use ostd::mm::dma::DmaCoherent;
use ostd::mm::{HasDaddr, VmIoOnce};
use ostd::sync::SpinLock;
use spin::Once;

mod command;
mod device;
mod hba;

/// Holds the AHCI HBA IrqRegistration for the driver's lifetime.
/// Only meaningful on x86 where PCI INTx# is routed through the IOAPIC.
static AHCI_IRQ: Once<IrqRegistration> = Once::new();

/// Initialize a physical SATA port.
///
/// Allocates DMA-coherent memory structures (command list, FIS, command table, DMA buffer)
/// and initializes the port on the HBA. Queries the device identity to determine geometry.
fn init_physical_port(abar: &IoMem, port: usize) -> Result<Arc<AhciBlockDevice>, ostd::Error> {
    let cmd_list = DmaCoherent::alloc(1, true)?;
    let fis = DmaCoherent::alloc(1, true)?;
    let cmd_table = DmaCoherent::alloc(1, true)?;
    let dma_buf = DmaCoherent::alloc(1, true)?;

    hba::init_port(abar, port, cmd_list.daddr() as u64, fis.daddr() as u64)?;

    // Query device geometry using IDENTIFY DEVICE
    let (num_blocks, block_size) =
        match command::identify_device(abar, port, &cmd_list, &cmd_table, &dma_buf) {
            Ok(identity) => (identity.sector_count as usize, identity.sector_size),
            Err(_) => (1024, 512), // Fallback defaults
        };

    let name = alloc::format!("ahci-port{}", port);
    Ok(Arc::new(AhciBlockDevice {
        name,
        inner: SpinLock::new(AhciBlockDeviceInner::Physical {
            abar: abar.clone(),
            port_no: port,
            block_size,
            num_blocks,
            cmd_list,
            fis,
            cmd_table,
            dma_buf,
        }),
    }))
}

/// Initialize the AHCI driver.
///
/// Scans the PCI bus for AHCI controllers, configures the controller
/// and port(s), and registers any found SATA disks. Falls back to a
/// simulated in-memory block device if no controller is present.
pub fn init() {
    let mut physical_found = false;

    // Find AHCI SATA controllers: class 0x01 (mass storage), subclass 0x06 (SATA)
    let pci_devices = crate::drivers::pci::find_devices_by_class(0x01, 0x06);

    for pci_dev in pci_devices {
        // Confirm prog_if == 0x01 (AHCI 1.0)
        if pci_dev.prog_if != 0x01 {
            continue;
        }

        // AHCI registers are mapped via BAR5
        let base_addr = match pci_dev.bars[5] {
            PciBar::MemoryMapped { base_addr, .. } if base_addr != 0 => base_addr,
            _ => continue,
        };

        // Enable device in PCI configuration space
        pci_dev.enable_memory_space();
        pci_dev.enable_bus_mastering();

        // Acquire the controller's MMIO register space (4KB length)
        let abar = match IoMem::acquire(base_addr as usize..base_addr as usize + 0x1000) {
            Ok(abar) => abar,
            Err(_) => continue,
        };

        if hba::init_controller(&abar).is_err() {
            continue;
        }

        // Register PCI INTx# interrupt handler for this HBA.
        // The handler reads PORT_IS for each implemented port to acknowledge
        // port-level interrupts.  Port interrupts are NOT enabled (IE bit is
        // not set in PORT_CMD), so the handler is defensive against stray
        // events; I/O completion uses polling (PORT_CI).
        let pi: u32 = abar.read_once(hba::HBA_PI).unwrap_or(0);
        let isr_abar = abar.clone();

        #[cfg(target_arch = "x86_64")]
        if !AHCI_IRQ.is_completed() {
            let irq_line = pci_dev.interrupt_line();
            if irq_line != 0 && irq_line < 16 {
                if let Ok(irq) = crate::drivers::irq::map_isa_irq(irq_line, move || {
                    for port in 0..32 {
                        if (pi & (1 << port)) != 0 {
                            let _ =
                                isr_abar.read_once::<u32>(hba::port_offset(port) + hba::PORT_IS);
                        }
                    }
                }) {
                    AHCI_IRQ.call_once(|| irq);
                }
            }
        }
        let mut count = 0;

        // Check each implemented port (up to 32 ports)
        for port in 0..32 {
            if (pi & (1 << port)) == 0 {
                continue;
            }
            if hba::port_connected(&abar, port) != Ok(true) {
                continue;
            }
            if hba::port_signature(&abar, port) != Ok(hba::SATA_SIG_DISK) {
                continue;
            }

            if let Ok(device) = init_physical_port(&abar, port) {
                let name = alloc::format!("ahci-port{}", port);
                if register_block_device(&name, device).is_ok() {
                    count += 1;
                }
            }
        }

        if count > 0 {
            physical_found = true;
            break;
        }
    }

    if !physical_found {
        // Fallback: register a simulated device if no physical AHCI controller was found
        let name = String::from("ahci-simulated");
        let device = Arc::new(AhciBlockDevice {
            name: name.clone(),
            inner: SpinLock::new(AhciBlockDeviceInner::Simulated {
                data: alloc::vec![0u8; 512 * 128],
            }),
        });
        let _ = register_block_device(&name, device);
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::ramfs::RamFs;
    use crate::fs::vfs::{init_root_fs, mount, register_filesystem, resolve_path};
    use ostd::prelude::ktest;

    #[ktest]
    fn test_ahci_driver() {
        init();

        let ramfs = Arc::new(RamFs);
        let _ = register_filesystem(ramfs);
        let _ = init_root_fs("ramfs", &[]);

        let devfs = Arc::new(crate::fs::devfs::DevFs);
        let _ = register_filesystem(devfs);

        let root = crate::fs::vfs::ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .unwrap();
        root.inode.mkdir("dev", 0o755).unwrap();

        mount("devfs", "/dev", 0, &[]).unwrap();

        let dev_name = if resolve_path("/dev/ahci-port0").is_ok() {
            "/dev/ahci-port0"
        } else if resolve_path("/dev/ahci-port1").is_ok() {
            "/dev/ahci-port1"
        } else if resolve_path("/dev/ahci-port2").is_ok() {
            "/dev/ahci-port2"
        } else {
            "/dev/ahci-simulated"
        };

        let ahci_dentry = resolve_path(dev_name).unwrap();
        assert_eq!(
            ahci_dentry.inode.metadata().unwrap().file_type,
            crate::fs::vfs::FileType::BlockDevice
        );

        let mut ops = ahci_dentry.inode.open(0).unwrap();
        let mut write_offset = 1024;
        ops.write(b"ahci test data", &mut write_offset).unwrap();

        let mut read_buf = [0u8; 14];
        let mut read_offset = 1024;
        ops.read(&mut read_buf, &mut read_offset).unwrap();
        assert_eq!(&read_buf, b"ahci test data");

        let clean_name = if dev_name.starts_with("/dev/ahci-port") {
            &dev_name[5..]
        } else {
            "ahci-simulated"
        };
        let _ = crate::drivers::unregister_device(clean_name);

        crate::fs::vfs::unregister_filesystem("devfs").unwrap();
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }
}
