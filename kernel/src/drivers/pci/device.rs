/// PCI Device Representation
///
/// Provides types for PCI devices and their Base Address Registers (BARs),
/// along with methods for reading/writing configuration space and
/// enabling device features.
use super::config;

// Standard PCI configuration space register offsets
const CFG_VENDOR_DEVICE: u8 = 0x00;
const CFG_COMMAND_STATUS: u8 = 0x04;
const CFG_CLASS_REV: u8 = 0x08;
const CFG_HEADER_TYPE: u8 = 0x0E;
const CFG_BAR0: u8 = 0x10;
const CFG_CAP_PTR: u8 = 0x34;
const CFG_INTERRUPT: u8 = 0x3C;

// Command register bits
const CMD_IO_SPACE: u16 = 1 << 0;
const CMD_MEMORY_SPACE: u16 = 1 << 1;
const CMD_BUS_MASTER: u16 = 1 << 2;

/// Represents a Base Address Register (BAR) on a PCI device.
///
/// BARs define the memory or I/O regions a device uses for communication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciBar {
    /// BAR slot is not used or not present.
    None,
    /// Memory-mapped BAR (MMIO region).
    MemoryMapped {
        base_addr: u64,
        size: u64,
        prefetchable: bool,
        is_64bit: bool,
    },
    /// I/O space BAR (port I/O region).
    IoSpace { port: u32, size: u32 },
}

/// A discovered PCI device with its configuration parsed from config space.
#[derive(Debug, Clone)]
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
    pub header_type: u8,
    pub bars: [PciBar; 6],
}

impl PciDevice {
    /// Read an 8-bit value from this device's configuration space.
    pub fn read_config_u8(&self, offset: u8) -> u8 {
        config::config_read_u8(self.bus, self.device, self.function, offset)
    }

    /// Read a 16-bit value from this device's configuration space.
    pub fn read_config_u16(&self, offset: u8) -> u16 {
        config::config_read_u16(self.bus, self.device, self.function, offset)
    }

    /// Read a 32-bit value from this device's configuration space.
    pub fn read_config_u32(&self, offset: u8) -> u32 {
        config::config_read_u32(self.bus, self.device, self.function, offset)
    }

    /// Write an 8-bit value to this device's configuration space.
    pub fn write_config_u8(&self, offset: u8, value: u8) {
        config::config_write_u8(self.bus, self.device, self.function, offset, value);
    }

    /// Write a 16-bit value to this device's configuration space.
    pub fn write_config_u16(&self, offset: u8, value: u16) {
        config::config_write_u16(self.bus, self.device, self.function, offset, value);
    }

    /// Write a 32-bit value to this device's configuration space.
    pub fn write_config_u32(&self, offset: u8, value: u32) {
        config::config_write_u32(self.bus, self.device, self.function, offset, value);
    }

    /// Read the PCI command register (offset 0x04, lower 16 bits).
    pub fn command(&self) -> u16 {
        self.read_config_u16(CFG_COMMAND_STATUS)
    }

    /// Read the PCI status register (offset 0x04, upper 16 bits).
    pub fn status(&self) -> u16 {
        self.read_config_u16(CFG_COMMAND_STATUS + 2)
    }

    /// Enable bus mastering (set bit 2 of the command register).
    ///
    /// This allows the device to initiate DMA transfers.
    pub fn enable_bus_mastering(&self) {
        let cmd = self.command();
        self.write_config_u16(CFG_COMMAND_STATUS, cmd | CMD_BUS_MASTER);
    }

    /// Enable memory space access (set bit 1 of the command register).
    ///
    /// This allows CPU access to the device's memory-mapped BARs.
    pub fn enable_memory_space(&self) {
        let cmd = self.command();
        self.write_config_u16(CFG_COMMAND_STATUS, cmd | CMD_MEMORY_SPACE);
    }

    /// Enable I/O space access (set bit 0 of the command register).
    pub fn enable_io_space(&self) {
        let cmd = self.command();
        self.write_config_u16(CFG_COMMAND_STATUS, cmd | CMD_IO_SPACE);
    }

    /// Read the interrupt line register (offset 0x3C).
    pub fn interrupt_line(&self) -> u8 {
        self.read_config_u8(CFG_INTERRUPT)
    }

    /// Read the interrupt pin register (offset 0x3D).
    pub fn interrupt_pin(&self) -> u8 {
        self.read_config_u8(CFG_INTERRUPT + 1)
    }

    /// Read the capabilities pointer (offset 0x34).
    pub fn capabilities_ptr(&self) -> u8 {
        self.read_config_u8(CFG_CAP_PTR) & 0xFC
    }
}

/// Probe a PCI location for a device, returning its parsed representation.
///
/// Returns `None` if no device is present (vendor ID is 0xFFFF).
pub(crate) fn probe(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let id_reg = config::config_read_u32(bus, device, function, CFG_VENDOR_DEVICE);
    let vendor_id = (id_reg & 0xFFFF) as u16;
    if vendor_id == 0xFFFF {
        return None;
    }
    let device_id = ((id_reg >> 16) & 0xFFFF) as u16;

    let class_rev = config::config_read_u32(bus, device, function, CFG_CLASS_REV);
    let revision = (class_rev & 0xFF) as u8;
    let prog_if = ((class_rev >> 8) & 0xFF) as u8;
    let subclass = ((class_rev >> 16) & 0xFF) as u8;
    let class_code = ((class_rev >> 24) & 0xFF) as u8;

    let header_type = config::config_read_u8(bus, device, function, CFG_HEADER_TYPE) & 0x7F;

    let bars = if header_type == 0x00 {
        parse_bars(bus, device, function)
    } else {
        [PciBar::None; 6]
    };

    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class_code,
        subclass,
        prog_if,
        revision,
        header_type,
        bars,
    })
}

/// Parse all 6 BARs for a Type 0 (endpoint) PCI device.
///
/// BAR sizing uses the standard write-all-ones-and-read-back technique:
/// 1. Save original BAR value
/// 2. Write 0xFFFFFFFF
/// 3. Read back to get size mask
/// 4. Restore original value
/// 5. Compute size from the mask
fn parse_bars(bus: u8, device: u8, function: u8) -> [PciBar; 6] {
    let mut bars = [PciBar::None; 6];
    let mut i = 0;

    while i < 6 {
        let offset = CFG_BAR0 + (i as u8) * 4;
        let original = config::config_read_u32(bus, device, function, offset);

        if original == 0 {
            i += 1;
            continue;
        }

        // Write all ones to determine size
        config::config_write_u32(bus, device, function, offset, 0xFFFF_FFFF);
        let size_mask = config::config_read_u32(bus, device, function, offset);
        // Restore original value
        config::config_write_u32(bus, device, function, offset, original);

        if size_mask == 0 {
            i += 1;
            continue;
        }

        if (original & 0x01) != 0 {
            // I/O space BAR
            let port = original & 0xFFFF_FFFC;
            let io_mask = size_mask & 0xFFFF_FFFC;
            let size = (!(io_mask) + 1) & 0xFFFF;
            bars[i] = PciBar::IoSpace {
                port,
                size: if size == 0 { 4 } else { size },
            };
        } else {
            // Memory BAR
            let bar_type = (original >> 1) & 0x03;
            let prefetchable = (original & 0x08) != 0;

            if bar_type == 0x02 && i < 5 {
                // 64-bit memory BAR
                let next_offset = CFG_BAR0 + ((i + 1) as u8) * 4;
                let original_high = config::config_read_u32(bus, device, function, next_offset);

                // Size the upper BAR
                config::config_write_u32(bus, device, function, next_offset, 0xFFFF_FFFF);
                let size_mask_high = config::config_read_u32(bus, device, function, next_offset);
                config::config_write_u32(bus, device, function, next_offset, original_high);

                let base_addr = ((original_high as u64) << 32) | ((original & 0xFFFF_FFF0) as u64);
                let full_mask =
                    ((size_mask_high as u64) << 32) | ((size_mask & 0xFFFF_FFF0) as u64);
                let size = if full_mask == 0 {
                    0
                } else {
                    (!full_mask).wrapping_add(1)
                };

                bars[i] = PciBar::MemoryMapped {
                    base_addr,
                    size,
                    prefetchable,
                    is_64bit: true,
                };
                // Next BAR slot consumed by the upper 32 bits
                bars[i + 1] = PciBar::None;
                i += 2;
                continue;
            } else {
                // 32-bit memory BAR
                let base_addr = (original & 0xFFFF_FFF0) as u64;
                let mem_mask = size_mask & 0xFFFF_FFF0;
                let size = if mem_mask == 0 {
                    0
                } else {
                    ((!mem_mask) as u64).wrapping_add(1)
                };

                bars[i] = PciBar::MemoryMapped {
                    base_addr,
                    size,
                    prefetchable,
                    is_64bit: false,
                };
            }
        }

        i += 1;
    }

    bars
}
