use crate::device::{Device, DeviceType, DriverError};

pub const PCI_VENDOR_NONE: u16 = 0xFFFF;

#[derive(Clone, Copy, Debug, Default)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
}

impl PciDevice {
    pub const fn new(
        bus: u8,
        device: u8,
        function: u8,
        vendor_id: u16,
        device_id: u16,
        class_code: u8,
        subclass: u8,
        prog_if: u8,
        revision: u8,
    ) -> Self {
        Self {
            bus,
            device,
            function,
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if,
            revision,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.vendor_id != PCI_VENDOR_NONE
    }

    pub fn class_name(&self) -> &'static str {
        match self.class_code {
            0x01 => "Mass storage controller",
            0x02 => "Network controller",
            0x03 => "Display controller",
            0x06 => "Bridge device",
            0x0C => "Serial bus controller",
            _ => "Unknown class",
        }
    }
}

impl Device for PciDevice {
    fn dev_type(&self) -> DeviceType {
        DeviceType::Bus
    }

    fn name(&self) -> &'static str {
        "PCI Bus Enumerator"
    }

    fn init(&mut self) -> Result<(), DriverError> {
        Ok(())
    }
}
