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

impl PciDiscovery {
    /// Return the slice of valid discovered devices.
    pub fn as_slice(&self) -> &[PciDevice] {
        &self.devices[..self.count]
    }

    /// Add a device to the discovery list.
    /// Returns `true` if added, or `false` if maximum capacity (256) is reached.
    pub fn push(&mut self, dev: PciDevice) -> bool {
        if self.count < self.devices.len() {
            self.devices[self.count] = dev;
            self.count += 1;
            true
        } else {
            false
        }
    }
}

pub struct PciBus;

impl PciBus {
    /// Check whether a device is present at the given BDF address.
    pub fn is_device_present(bus: u8, device: u8, function: u8) -> bool {
        config::read_u16(bus, device, function, 0x00) != PCI_VENDOR_NONE
    }

    /// Probe a specific BDF function. Returns `None` if device is absent.
    pub fn probe_device(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
        let vendor_id = config::read_u16(bus, device, function, 0x00);
        if vendor_id == PCI_VENDOR_NONE {
            return None;
        }

        let device_id = config::read_u16(bus, device, function, 0x02);
        let class_code = config::read_u8(bus, device, function, 0x0B);
        let subclass = config::read_u8(bus, device, function, 0x0A);
        let prog_if = config::read_u8(bus, device, function, 0x09);
        let revision = config::read_u8(bus, device, function, 0x08);

        Some(PciDevice::new(
            bus, device, function, vendor_id, device_id, class_code, subclass, prog_if, revision,
        ))
    }

    /// Enumerate all devices across the PCI bus topology.
    ///
    /// Optimizations:
    /// - If Function 0 is absent, skips remaining functions (1..7) for that device slot.
    /// - If Function 0 indicates single-function in Header Type (bit 7 == 0), skips functions 1..7.
    /// - Bounds-checked insertion with `push` prevents kernel panics on capacity overflow.
    pub fn enumerate() -> PciDiscovery {
        let mut discovery = PciDiscovery::default();

        for bus in 0..=255u8 {
            for device in 0..32u8 {
                // Probe function 0 first to check if device slot is occupied
                let Some(fn0) = Self::probe_device(bus, device, 0) else {
                    continue;
                };

                if !discovery.push(fn0) {
                    log::warn!(
                        "PCI: Maximum device capacity ({}) reached during enumeration",
                        discovery.devices.len()
                    );
                    return discovery;
                }

                // Check Header Type (offset 0x0E): bit 7 is set for multi-function devices
                let header_type = config::read_u8(bus, device, 0, 0x0E);
                if (header_type & 0x80) == 0 {
                    // Single-function device: skip functions 1..7
                    continue;
                }

                // Multi-function device: probe functions 1..7
                for function in 1..8u8 {
                    if let Some(info) = Self::probe_device(bus, device, function) {
                        if !discovery.push(info) {
                            log::warn!(
                                "PCI: Maximum device capacity ({}) reached during enumeration",
                                discovery.devices.len()
                            );
                            return discovery;
                        }
                    }
                }
            }
        }

        discovery
    }
}
