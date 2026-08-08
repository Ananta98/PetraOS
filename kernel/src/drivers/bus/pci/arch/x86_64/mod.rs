//! PCI Configuration Space Access for x86_64 Architecture
//!
//! Accesses PCI configuration space via I/O ports 0xCF8 (Address) and 0xCFC (Data).

use crate::arch::ports::Ports;

pub const PCI_CONFIG_ADDRESS_PORT: u16 = 0xCF8;
pub const PCI_CONFIG_DATA_PORT: u16 = 0xCFC;
pub const MSI_ADDR_BASE: u32 = 0xFEE0_0000;

/// Construct an x86_64 MSI Message Address targeting a specific CPU APIC ID.
pub fn msi_address(apic_id: u8) -> u32 {
    MSI_ADDR_BASE | ((apic_id as u32) << 12)
}

/// Construct an x86_64 MSI Message Data value for a given APIC interrupt vector.
pub fn msi_data(vector: u8) -> u16 {
    vector as u16
}

/// Build the 32-bit PCI configuration address for the given BDF and register offset.
pub fn make_address(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    0x8000_0000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((func as u32) << 8)
        | (u32::from(offset & 0xFC))
}

/// Write configuration address register to 0xCF8.
pub fn write_address(bus: u8, device: u8, func: u8, offset: u8) {
    let address = make_address(bus, device, func, offset);
    unsafe {
        Ports::outl(PCI_CONFIG_ADDRESS_PORT, address);
    }
}

/// Read a 32-bit dword from PCI configuration space.
pub fn read_u32(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    write_address(bus, device, func, offset);
    unsafe { Ports::inl(PCI_CONFIG_DATA_PORT) }
}

/// Read a 16-bit word from PCI configuration space.
pub fn read_u16(bus: u8, device: u8, func: u8, offset: u8) -> u16 {
    (read_u32(bus, device, func, offset) >> ((offset & 0x02) * 8)) as u16
}

/// Read an 8-bit byte from PCI configuration space.
pub fn read_u8(bus: u8, device: u8, func: u8, offset: u8) -> u8 {
    (read_u32(bus, device, func, offset) >> ((offset & 0x03) * 8)) as u8
}

/// Write a 32-bit dword to PCI configuration space.
pub fn write_u32(bus: u8, device: u8, func: u8, offset: u8, value: u32) {
    write_address(bus, device, func, offset);
    unsafe {
        Ports::outl(PCI_CONFIG_DATA_PORT, value);
    }
}

/// Write a 16-bit word to PCI configuration space.
pub fn write_u16(bus: u8, device: u8, func: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let mut dword = read_u32(bus, device, func, aligned);
    let shift = ((offset & 0x02) as u32) * 8;
    dword &= !(0xFFFF << shift);
    dword |= (value as u32) << shift;
    write_u32(bus, device, func, aligned, dword);
}

/// Write an 8-bit byte to PCI configuration space.
pub fn write_u8(bus: u8, device: u8, func: u8, offset: u8, value: u8) {
    let aligned = offset & 0xFC;
    let mut dword = read_u32(bus, device, func, aligned);
    let shift = ((offset & 0x03) as u32) * 8;
    dword &= !(0xFF << shift);
    dword |= (value as u32) << shift;
    write_u32(bus, device, func, aligned, dword);
}
