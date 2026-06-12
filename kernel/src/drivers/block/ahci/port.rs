use super::hba::HbaPort;

pub const SATA_SIG_ATA: u32 = 0x00000101;
pub const SATA_SIG_ATAPI: u32 = 0xEB140101;
pub const SATA_SIG_SEMB: u32 = 0xC33C0101;
pub const SATA_SIG_PM: u32 = 0x96690101;

pub const HBA_PORT_IPM_ACTIVE: u32 = 1;
pub const HBA_PORT_DET_PRESENT: u32 = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum AhciDeviceType {
    None,
    SATA,
    SATAPI,
    SEMB,
    PM,
    Unknown,
}

pub fn check_device_type(port: &HbaPort) -> AhciDeviceType {
    let ssts = unsafe { core::ptr::read_volatile(&port.ssts) };

    let ipm = (ssts >> 8) & 0x0F;
    let det = ssts & 0x0F;

    if det != HBA_PORT_DET_PRESENT || ipm != HBA_PORT_IPM_ACTIVE {
        return AhciDeviceType::None;
    }

    let sig = unsafe { core::ptr::read_volatile(&port.sig) };
    match sig {
        SATA_SIG_ATA => AhciDeviceType::SATA,
        SATA_SIG_ATAPI => AhciDeviceType::SATAPI,
        SATA_SIG_SEMB => AhciDeviceType::SEMB,
        SATA_SIG_PM => AhciDeviceType::PM,
        _ => AhciDeviceType::Unknown,
    }
}
