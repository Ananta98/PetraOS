use core::arch::asm;

pub const PCI_CONFIG_ADDRESS_PORT: u16 = 0xCF8;
pub const PCI_CONFIG_DATA_PORT: u16 = 0xCFC;

pub struct PciConfig;

impl PciConfig {
    pub fn write_address(bus: u8, device: u8, function: u8, offset: u8) {
        let address = 0x8000_0000u32
            | ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | (u32::from(offset & 0xFC));

        unsafe {
            asm!(
                "out dx, eax",
                in("dx") PCI_CONFIG_ADDRESS_PORT,
                in("eax") address,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    pub fn read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
        Self::write_address(bus, device, function, offset);
        let value: u32;

        unsafe {
            asm!(
                "in eax, dx",
                in("dx") PCI_CONFIG_DATA_PORT,
                out("eax") value,
                options(nomem, nostack, preserves_flags)
            );
        }

        value
    }

    pub fn read_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
        (Self::read_u32(bus, device, function, offset) >> ((offset & 0x02) * 8)) as u16
    }

    pub fn read_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
        (Self::read_u32(bus, device, function, offset) >> ((offset & 0x03) * 8)) as u8
    }
}
