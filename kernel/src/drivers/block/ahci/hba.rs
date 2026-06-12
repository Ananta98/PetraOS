#[repr(C)]
pub struct HbaPort {
    pub clb: u32,       // Command list base address, 1K-byte aligned
    pub clbu: u32,      // Command list base address upper 32 bits
    pub fb: u32,        // FIS base address, 256-byte aligned
    pub fbu: u32,       // FIS base address upper 32 bits
    pub is: u32,        // Interrupt status
    pub ie: u32,        // Interrupt enable
    pub cmd: u32,       // Command and status
    pub rsv0: u32,
    pub tfd: u32,       // Task file data
    pub sig: u32,       // Signature
    pub ssts: u32,      // SATA status
    pub sctl: u32,      // SATA control
    pub serr: u32,      // SATA error
    pub sact: u32,      // SATA active
    pub ci: u32,        // Command issue
    pub sntf: u32,      // SATA notification
    pub fbs: u32,       // FIS-based switch control
    pub rsv1: [u32; 11],
    pub vendor: [u32; 4],
}

#[repr(C)]
pub struct HbaMem {
    pub cap: u32,       // Host capability
    pub ghc: u32,       // Global host control
    pub is: u32,        // Interrupt status
    pub pi: u32,        // Port implemented
    pub vs: u32,        // Version
    pub ccc_ctl: u32,   // Command completion coalescing control
    pub ccc_pts: u32,   // Command completion coalescing ports
    pub em_loc: u32,    // Enclosure management location
    pub em_ctl: u32,    // Enclosure management control
    pub cap2: u32,      // Host capabilities extended
    pub bohc: u32,      // BIOS/OS handoff control and status
    pub rsv: [u8; 0x74],
    pub vendor: [u8; 0x60],
    pub ports: [HbaPort; 32], // 1 to 32
}
