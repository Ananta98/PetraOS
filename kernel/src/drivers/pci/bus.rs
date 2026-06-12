use crate::drivers::{DriverError, Device};
use super::device::{PciDevice, PCI_VENDOR_NONE};
use super::config::PciConfig;

#[derive(Clone, Copy, Debug)]
pub struct PciDiscovery {
    pub devices: [PciDevice; 256],
    pub count: usize,
}

impl Default for PciDiscovery {
    fn default() -> Self {
        Self {
            devices: [PciDevice::default(); 256],
            count: 0,
        }
    }
}

pub struct PciBus;

impl PciBus {
    pub fn is_device_present(bus: u8, device: u8, function: u8) -> bool {
        PciConfig::read_u16(bus, device, function, 0x00) != PCI_VENDOR_NONE
    }

    pub fn probe_device(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
        if !Self::is_device_present(bus, device, function) {
            return None;
        }

        let vendor_id = PciConfig::read_u16(bus, device, function, 0x00);
        let device_id = PciConfig::read_u16(bus, device, function, 0x02);
        let class_code = PciConfig::read_u8(bus, device, function, 0x0B);
        let subclass = PciConfig::read_u8(bus, device, function, 0x0A);
        let prog_if = PciConfig::read_u8(bus, device, function, 0x09);
        let revision = PciConfig::read_u8(bus, device, function, 0x08);

        Some(PciDevice::new(bus, device, function, vendor_id, device_id, class_code, subclass, prog_if, revision))
    }

    pub fn enumerate() -> PciDiscovery {
        let mut discovery = PciDiscovery::default();

        for bus in 0..=255u8 {
            for device in 0..32u8 {
                for function in 0..8u8 {
                    let Some(info) = Self::probe_device(bus, device, function) else {
                        continue;
                    };

                    if !info.is_valid() {
                        continue;
                    }

                    discovery.devices[discovery.count] = info;
                    discovery.count += 1;
                }
            }
        }

        discovery
    }

    pub fn init() -> Result<(), DriverError> {
        let discovery = Self::enumerate();

        if discovery.count == 0 {
            log::info!("PCI: no devices discovered");
            return Ok(());
        }

        for entry in &discovery.devices[..discovery.count] {
            log::info!(
                "PCI: bus={} dev={} func={} vendor={:04x} device={:04x} class={:02x} subclass={:02x} ({})",
                entry.bus,
                entry.device,
                entry.function,
                entry.vendor_id,
                entry.device_id,
                entry.class_code,
                entry.subclass,
                entry.class_name()
            );

            if entry.class_code == 0x01 && entry.subclass == 0x06 {
                let mut ahci = crate::drivers::block::ahci::AhciDriver::new(*entry);
                if ahci.init().is_ok() {
                    *crate::drivers::block::ahci::AHCI_DEVICE.lock() = Some(ahci);
                    let device_ref: alloc::sync::Arc<crate::sync::spinlock::Spinlock<alloc::boxed::Box<dyn Device>>> = alloc::sync::Arc::new(crate::sync::spinlock::Spinlock::new(
                        alloc::boxed::Box::new(crate::drivers::block::ahci::AhciDeviceRef)
                    ));
                    crate::drivers::DEVICE_MANAGER.lock().register(device_ref);
                    log::info!("PCI: AHCI device initialized and registered to DEVICE_MANAGER");
                }
            } else if entry.class_code == 0x01 && entry.subclass == 0x08 {
                let mut nvme = crate::drivers::block::nvme::NvmeDriver::new(*entry);
                if nvme.init().is_ok() {
                    *crate::drivers::block::nvme::NVME_DEVICE.lock() = Some(nvme);
                    let device_ref: alloc::sync::Arc<crate::sync::spinlock::Spinlock<alloc::boxed::Box<dyn Device>>> = alloc::sync::Arc::new(crate::sync::spinlock::Spinlock::new(
                        alloc::boxed::Box::new(crate::drivers::block::nvme::NvmeDeviceRef)
                    ));
                    crate::drivers::DEVICE_MANAGER.lock().register(device_ref);
                    log::info!("PCI: NVMe device initialized and registered to DEVICE_MANAGER");
                }
            }
        }

        Ok(())
    }
}
