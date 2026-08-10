use super::arch as config;
use super::device::{PCI_VENDOR_NONE, PciDevice};

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
        config::read_u16(bus, device, function, 0x00) != PCI_VENDOR_NONE
    }

    pub fn probe_device(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
        if !Self::is_device_present(bus, device, function) {
            return None;
        }

        let vendor_id = config::read_u16(bus, device, function, 0x00);
        let device_id = config::read_u16(bus, device, function, 0x02);
        let class_code = config::read_u8(bus, device, function, 0x0B);
        let subclass = config::read_u8(bus, device, function, 0x0A);
        let prog_if = config::read_u8(bus, device, function, 0x09);
        let revision = config::read_u8(bus, device, function, 0x08);

        Some(PciDevice::new(
            bus, device, function, vendor_id, device_id, class_code, subclass, prog_if, revision,
        ))
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
}
