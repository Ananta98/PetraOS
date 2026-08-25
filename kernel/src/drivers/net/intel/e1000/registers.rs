//! Intel 8254x (e1000) Network Controller Register Definitions
//!
//! Provides register offset constants and bitflags for Device Control,
//! Receive/Transmit Control, EEPROM, and Interrupts.

// ===== Register Offsets (relative to BAR0 MMIO base) =====

pub const REG_CTRL: usize = 0x0000;
pub const REG_STATUS: usize = 0x0008;
pub const REG_EECD: usize = 0x0010;
pub const REG_EERD: usize = 0x0014;
pub const REG_FLA: usize = 0x001C;
pub const REG_CTRL_EXT: usize = 0x0018;
pub const REG_MDIC: usize = 0x0020;
pub const REG_ICR: usize = 0x00C0;
pub const REG_ITR: usize = 0x00C4;
pub const REG_ICS: usize = 0x00C8;
pub const REG_IMS: usize = 0x00D0;
pub const REG_IMC: usize = 0x00D8;
pub const REG_RCTL: usize = 0x0100;
pub const REG_FCTTV: usize = 0x0170;
pub const REG_TXCW: usize = 0x0178;
pub const REG_RXCW: usize = 0x0180;
pub const REG_TCTL: usize = 0x0400;
pub const REG_TIPG: usize = 0x0410;
pub const REG_AIT: usize = 0x0458;
pub const REG_LEDCTL: usize = 0x0E00;
pub const REG_PBA: usize = 0x1000;
pub const REG_RDBAL: usize = 0x2800;
pub const REG_RDBAH: usize = 0x2804;
pub const REG_RDLEN: usize = 0x2808;
pub const REG_RDH: usize = 0x2810;
pub const REG_RDT: usize = 0x2818;
pub const REG_RDTR: usize = 0x2820;
pub const REG_RXDCTL: usize = 0x2828;
pub const REG_RADV: usize = 0x282C;
pub const REG_RSRPD: usize = 0x2C00;
pub const REG_TDBAL: usize = 0x3800;
pub const REG_TDBAH: usize = 0x3804;
pub const REG_TDLEN: usize = 0x3808;
pub const REG_TDH: usize = 0x3810;
pub const REG_TDT: usize = 0x3818;
pub const REG_TIDV: usize = 0x3820;
pub const REG_TXDCTL: usize = 0x3828;
pub const REG_TADV: usize = 0x382C;
pub const REG_TSPMT: usize = 0x3830;
pub const REG_MTA: usize = 0x5200; // Multicast Table Array (128 dwords, 5200h-53FC)
pub const REG_RAL: usize = 0x5400; // Receive Address Low
pub const REG_RAH: usize = 0x5404; // Receive Address High

// ===== Device Control (CTRL) Bits =====

pub const CTRL_FD: u32 = 1 << 0; // Full Duplex
pub const CTRL_ASDE: u32 = 1 << 5; // Auto-Speed Detection Enable
pub const CTRL_SLU: u32 = 1 << 6; // Set Link Up
pub const CTRL_ILOS: u32 = 1 << 7; // Invert Loss-of-Signal
pub const CTRL_SPEED_100: u32 = 1 << 8;
pub const CTRL_SPEED_1000: u32 = 2 << 8;
pub const CTRL_FRCSPD: u32 = 1 << 11; // Force Speed
pub const CTRL_FRCDPLX: u32 = 1 << 12; // Force Duplex
pub const CTRL_RST: u32 = 1 << 26; // Device Reset
pub const CTRL_RFCE: u32 = 1 << 27; // Receive Flow Control Enable
pub const CTRL_TFCE: u32 = 1 << 28; // Transmit Flow Control Enable
pub const CTRL_VME: u32 = 1 << 30; // VLAN Mode Enable
pub const CTRL_PHY_RST: u32 = 1 << 31; // PHY Reset

// ===== Device Status (STATUS) Bits =====

pub const STATUS_FD: u32 = 1 << 0; // Full Duplex
pub const STATUS_LU: u32 = 1 << 1; // Link Up
pub const STATUS_SPEED_100: u32 = 1 << 6;
pub const STATUS_SPEED_1000: u32 = 2 << 6;

// ===== EEPROM Read (EERD) Bits =====

pub const EERD_START: u32 = 1 << 0; // Start Read
pub const EERD_DONE: u32 = 1 << 4; // Read Done (82544 and older)
pub const EERD_DONE_NEW: u32 = 1 << 1; // Read Done (82540 and newer)
pub const EERD_ADDR_SHIFT: u32 = 8; // Address shift for 82544
pub const EERD_ADDR_SHIFT_NEW: u32 = 2; // Address shift for 82540
pub const EERD_DATA_SHIFT: u32 = 16; // Read data shift

// ===== Receive Control (RCTL) Bits =====

pub const RCTL_EN: u32 = 1 << 1; // Receiver Enable
pub const RCTL_SBP: u32 = 1 << 2; // Store Bad Packets
pub const RCTL_UPE: u32 = 1 << 3; // Unicast Promiscuous Enabled
pub const RCTL_MPE: u32 = 1 << 4; // Multicast Promiscuous Enabled
pub const RCTL_LPE: u32 = 1 << 5; // Long Packet Reception Enable
pub const RCTL_LBM_NONE: u32 = 0 << 6; // No Loopback
pub const RCTL_RDMTS_HALF: u32 = 0 << 8; // Rx Descriptor Minimum Threshold Size (1/2 RDLEN)
pub const RCTL_MO_36: u32 = 0 << 12; // Multicast Offset
pub const RCTL_BAM: u32 = 1 << 15; // Broadcast Accept Mode
pub const RCTL_BSIZE_2048: u32 = 0 << 16; // Receive Buffer Size 2048 bytes
pub const RCTL_BSIZE_1024: u32 = 1 << 16;
pub const RCTL_BSIZE_512: u32 = 2 << 16;
pub const RCTL_BSIZE_256: u32 = 3 << 16;
pub const RCTL_BSIZE_16384: u32 = (1 << 16) | (1 << 25);
pub const RCTL_BSIZE_8192: u32 = (2 << 16) | (1 << 25);
pub const RCTL_BSIZE_4096: u32 = (3 << 16) | (1 << 25);
pub const RCTL_DPF: u32 = 1 << 22; // Discard Pause Frames
pub const RCTL_PMCF: u32 = 1 << 23; // Pass MAC Control Frames
pub const RCTL_BSEX: u32 = 1 << 25; // Buffer Size Extension
pub const RCTL_SECRC: u32 = 1 << 26; // Strip Ethernet CRC

// ===== Transmit Control (TCTL) Bits =====

pub const TCTL_EN: u32 = 1 << 1; // Transmitter Enable
pub const TCTL_PSP: u32 = 1 << 3; // Pad Short Packets
pub const TCTL_CT_SHIFT: u32 = 4; // Collision Threshold shift
pub const TCTL_COLD_SHIFT: u32 = 12; // Collision Distance shift
pub const TCTL_SWXOFF: u32 = 1 << 22; // Software XOFF Transmission
pub const TCTL_RTLC: u32 = 1 << 24; // Re-transmit on Late Collision

// ===== Interrupt Masks & Causes (IMS / ICR) =====

pub const INT_TXDW: u32 = 1 << 0; // Transmit Descriptor Written Back
pub const INT_TXQE: u32 = 1 << 1; // Transmit Queue Empty
pub const INT_LSC: u32 = 1 << 2; // Link Status Change
pub const INT_RXSEQ: u32 = 1 << 3; // Receive Sequence Error
pub const INT_RXDMT0: u32 = 1 << 4; // Receive Descriptor Minimum Threshold Reached
pub const INT_RXO: u32 = 1 << 6; // Receiver Overrun
pub const INT_RXT0: u32 = 1 << 7; // Receiver Timer Interrupt
pub const INT_MDAC: u32 = 1 << 9; // MDI/O Access Complete
pub const INT_RXCFG: u32 = 1 << 10; // Receiving /C/ ordered sets
pub const INT_PHYINT: u32 = 1 << 12; // PHY Interrupt
pub const INT_TXDLOW: u32 = 1 << 15; // Transmit Descriptor Low
pub const INT_SRPD: u32 = 1 << 16; // Small Receive Packet Detected
pub const INT_ALL: u32 = 0xFFFF_FFFF;

// ===== Receive Address High (RAH) Bits =====

pub const RAH_AV: u32 = 1 << 31; // Address Valid
