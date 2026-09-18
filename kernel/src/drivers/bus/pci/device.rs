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

    /// Read an 8-bit byte from this device's PCI configuration space.
    pub fn read_u8(&self, offset: u8) -> u8 {
        super::config::read_u8(self.bus, self.device, self.function, offset)
    }

    /// Read a 16-bit word from this device's PCI configuration space.
    pub fn read_u16(&self, offset: u8) -> u16 {
        super::config::read_u16(self.bus, self.device, self.function, offset)
    }

    /// Read a 32-bit dword from this device's PCI configuration space.
    pub fn read_u32(&self, offset: u8) -> u32 {
        super::config::read_u32(self.bus, self.device, self.function, offset)
    }

    /// Write an 8-bit byte to this device's PCI configuration space.
    pub fn write_u8(&self, offset: u8, value: u8) {
        super::config::write_u8(self.bus, self.device, self.function, offset, value);
    }

    /// Write a 16-bit word to this device's PCI configuration space.
    pub fn write_u16(&self, offset: u8, value: u16) {
        super::config::write_u16(self.bus, self.device, self.function, offset, value);
    }

    /// Write a 32-bit dword to this device's PCI configuration space.
    pub fn write_u32(&self, offset: u8, value: u32) {
        super::config::write_u32(self.bus, self.device, self.function, offset, value);
    }

    /// Read the 16-bit Command register.
    pub fn command(&self) -> u16 {
        self.read_u16(0x04)
    }

    /// Write to the 16-bit Command register.
    pub fn set_command(&self, cmd: u16) {
        self.write_u16(0x04, cmd);
    }

    /// Read the 16-bit Status register.
    pub fn status(&self) -> u16 {
        self.read_u16(0x06)
    }

    /// Enable Bus Master (bit 2) in the PCI Command register.
    pub fn enable_bus_master(&self) {
        let cmd = self.command();
        self.set_command(cmd | 0x0004);
    }

    /// Enable Memory Space access (bit 1) in the PCI Command register.
    pub fn enable_memory_space(&self) {
        let cmd = self.command();
        self.set_command(cmd | 0x0002);
    }

    /// Read the 8-bit Interrupt Line register (offset 0x3C).
    pub fn interrupt_line(&self) -> u8 {
        self.read_u8(0x3C)
    }

    /// Read the 8-bit Interrupt Pin register (offset 0x3D).
    /// Returns 0 if INTx is not used, or 1=INTA#, 2=INTB#, 3=INTC#, 4=INTD#.
    pub fn interrupt_pin(&self) -> u8 {
        self.read_u8(0x3D)
    }

    /// Enable legacy INTx pin interrupt assertion (clears bit 10 "Interrupt Disable" in Command register).
    pub fn enable_intx(&self) {
        let cmd = self.command();
        self.set_command(cmd & !(1 << 10));
    }

    /// Check if the device implements a PCI capabilities list (Status register bit 4).
    pub fn has_capabilities_list(&self) -> bool {
        (self.status() & (1 << 4)) != 0
    }

    /// Search the PCI capabilities linked list for the specified capability ID.
    /// Returns the configuration space offset of the capability if found.
    pub fn find_capability(&self, cap_id: u8) -> Option<u8> {
        if !self.has_capabilities_list() {
            return None;
        }

        let mut offset = self.read_u8(0x34) & 0xFC;
        let mut hops = 0;

        while offset >= 0x40 && offset <= 0xFC && hops < 48 {
            let current_id = self.read_u8(offset);
            if current_id == cap_id {
                return Some(offset);
            }
            offset = self.read_u8(offset + 1) & 0xFC;
            hops += 1;
        }

        None
    }

    /// Check whether a capability ID is present in the device's capability list.
    pub fn has_capability(&self, cap_id: u8) -> bool {
        self.find_capability(cap_id).is_some()
    }

    /// Resolve the physical base address for a memory BAR (Base Indicator Register: 0..5).
    /// Safely decodes both 32-bit and 64-bit Memory Space BARs.
    pub fn bar_phys(&self, bir: u8) -> Option<u64> {
        if bir > 5 {
            return None;
        }

        let offset = 0x10 + (bir * 4);
        let bar_lo = self.read_u32(offset);
        if bar_lo == 0 || bar_lo == 0xFFFF_FFFF {
            return None;
        }

        // Bit 0 must be 0 for Memory Space BAR
        if (bar_lo & 0x01) != 0 {
            return None;
        }

        let bar_type = (bar_lo >> 1) & 0x03;
        match bar_type {
            0b00 => {
                // 32-bit Memory Space BAR
                Some((bar_lo & !0x0F) as u64)
            }
            0b10 => {
                // 64-bit Memory Space BAR
                if bir >= 5 {
                    return None;
                }
                let bar_hi = self.read_u32(offset + 4);
                Some(((bar_hi as u64) << 32) | ((bar_lo & !0x0F) as u64))
            }
            _ => None,
        }
    }

    /// Configure interrupts for this PCI device:
    /// - If the device supports MSI-X, attempts to initialize and configure MSI-X vector 0.
    /// - If MSI-X is unsupported or fails, gracefully falls back to the legacy PCI INTx interrupt line.
    pub fn setup_interrupts(&self, preferred_vector: u8, masked: bool) -> PciInterruptMode {
        if self.has_capability(super::msix::PCI_CAP_ID_MSIX) {
            match super::msix::init_device(self) {
                Ok(mut msix) => {
                    log::info!(
                        "PCI [{:02x}:{:02x}.{}]: MSI-X capability detected ({} vectors)",
                        self.bus,
                        self.device,
                        self.function,
                        msix.table_size()
                    );
                    if msix
                        .configure_vector(0, 0, preferred_vector, masked)
                        .is_ok()
                    {
                        msix.enable();
                        log::info!(
                            "PCI [{:02x}:{:02x}.{}]: configured and enabled MSI-X vector 0 (CPU vector 0x{:02x}, masked={})",
                            self.bus,
                            self.device,
                            self.function,
                            preferred_vector,
                            masked
                        );
                        return PciInterruptMode::Msix(msix);
                    }
                    log::warn!(
                        "PCI [{:02x}:{:02x}.{}]: MSI-X vector configuration failed; falling back to legacy PCI",
                        self.bus,
                        self.device,
                        self.function
                    );
                }
                Err(e) => {
                    log::warn!(
                        "PCI [{:02x}:{:02x}.{}]: MSI-X initialization failed ({:?}); falling back to legacy PCI",
                        self.bus,
                        self.device,
                        self.function,
                        e
                    );
                }
            }
        }

        let pin = self.interrupt_pin();
        let line = self.interrupt_line();
        if pin != 0 {
            self.enable_intx();
            log::info!(
                "PCI [{:02x}:{:02x}.{}]: Using legacy PCI INTx interrupt (pin={}, line={})",
                self.bus,
                self.device,
                self.function,
                pin,
                line
            );
            PciInterruptMode::LegacyIntx { line, pin }
        } else {
            log::info!(
                "PCI [{:02x}:{:02x}.{}]: No MSI-X and no INTx pin assigned; operating in polling mode",
                self.bus,
                self.device,
                self.function
            );
            PciInterruptMode::None
        }
    }
}

/// Interrupt operating mode for a PCI device.
#[derive(Debug)]
pub enum PciInterruptMode {
    /// Modern MSI-X message-signaled interrupts with dedicated vectors.
    Msix(super::msix::MsixCapability),
    /// Legacy PCI INTx pin-based interrupt line (classic PCI fallback).
    LegacyIntx {
        /// System interrupt line (IRQ line from config space offset 0x3C).
        line: u8,
        /// Physical interrupt pin (INTA#=1, INTB#=2, INTC#=3, INTD#=4 from 0x3D).
        pin: u8,
    },
    /// Polling mode / no hardware interrupts configured.
    None,
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
