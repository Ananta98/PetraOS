/// PCI Configuration Space Access
///
/// On x86_64, PCI configuration space is accessed via I/O port mechanism 1:
/// - Address port (0xCF8): Selects bus/device/function/register
/// - Data port (0xCFC): Reads/writes the selected register
///
/// Address format:
/// ```text
/// [31]    Enable bit (must be 1)
/// [23:16] Bus number (0-255)
/// [15:11] Device number (0-31)
/// [10:8]  Function number (0-7)
/// [7:2]   Register number (offset >> 2)
/// [1:0]   Always 0 (dword aligned)
/// ```

use ostd::arch::device::io_port::ReadWriteAccess;
use ostd::io::IoPort;

/// Build the PCI configuration address for the given BDF and register offset.
fn make_address(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC)
}

/// Read a 32-bit value from PCI configuration space.
pub fn config_read_u32(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    let address = make_address(bus, device, func, offset);
    if let (Ok(addr_port), Ok(data_port)) = (
        IoPort::<u32, ReadWriteAccess>::acquire(0xCF8),
        IoPort::<u32, ReadWriteAccess>::acquire(0xCFC),
    ) {
        addr_port.write(address);
        data_port.read()
    } else {
        0xFFFF_FFFF
    }
}

/// Read a 16-bit value from PCI configuration space.
pub fn config_read_u16(bus: u8, device: u8, func: u8, offset: u8) -> u16 {
    let dword = config_read_u32(bus, device, func, offset & 0xFC);
    let shift = ((offset & 0x02) as u32) * 8;
    ((dword >> shift) & 0xFFFF) as u16
}

/// Read an 8-bit value from PCI configuration space.
pub fn config_read_u8(bus: u8, device: u8, func: u8, offset: u8) -> u8 {
    let dword = config_read_u32(bus, device, func, offset & 0xFC);
    let shift = ((offset & 0x03) as u32) * 8;
    ((dword >> shift) & 0xFF) as u8
}

/// Write a 32-bit value to PCI configuration space.
pub fn config_write_u32(bus: u8, device: u8, func: u8, offset: u8, value: u32) {
    let address = make_address(bus, device, func, offset);
    if let (Ok(addr_port), Ok(data_port)) = (
        IoPort::<u32, ReadWriteAccess>::acquire(0xCF8),
        IoPort::<u32, ReadWriteAccess>::acquire(0xCFC),
    ) {
        addr_port.write(address);
        data_port.write(value);
    }
}

/// Write a 16-bit value to PCI configuration space.
pub fn config_write_u16(bus: u8, device: u8, func: u8, offset: u8, value: u16) {
    let aligned = offset & 0xFC;
    let mut dword = config_read_u32(bus, device, func, aligned);
    let shift = ((offset & 0x02) as u32) * 8;
    dword &= !(0xFFFF << shift);
    dword |= (value as u32) << shift;
    config_write_u32(bus, device, func, aligned, dword);
}

/// Write an 8-bit value to PCI configuration space.
pub fn config_write_u8(bus: u8, device: u8, func: u8, offset: u8, value: u8) {
    let aligned = offset & 0xFC;
    let mut dword = config_read_u32(bus, device, func, aligned);
    let shift = ((offset & 0x03) as u32) * 8;
    dword &= !(0xFF << shift);
    dword |= (value as u32) << shift;
    config_write_u32(bus, device, func, aligned, dword);
}