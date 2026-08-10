//! NVMe Command Structures and Admin/IO Opcodes

use super::queue::NvmeCmd;

// Admin Opcodes
pub const NVME_ADMIN_DELETE_SQ: u8 = 0x00;
pub const NVME_ADMIN_CREATE_SQ: u8 = 0x01;
pub const NVME_ADMIN_DELETE_CQ: u8 = 0x04;
pub const NVME_ADMIN_CREATE_CQ: u8 = 0x05;
pub const NVME_ADMIN_IDENTIFY: u8 = 0x06;

// NVM I/O Opcodes
pub const NVME_NVM_WRITE: u8 = 0x01;
pub const NVME_NVM_READ: u8 = 0x02;

// Identify CNS (Controller / Namespace Structure) values
pub const NVME_IDENTIFY_CNS_NS: u32 = 0x00;
pub const NVME_IDENTIFY_CNS_CTRL: u32 = 0x01;

/// NVMe Identify Namespace Data Structure (4096 bytes)
#[repr(C, packed)]
pub struct NvmeIdentifyNamespace {
    pub nsze: u64, // Namespace Size in logical blocks
    pub ncap: u64, // Namespace Capacity
    pub nuse: u64, // Namespace Utilization
    pub nsfeat: u8,
    pub nlbaf: u8, // Number of LBA Formats
    pub flbas: u8, // Formatted LBA Size
    pub mc: u8,
    pub dpc: u8,
    pub dps: u8,
    pub nmic: u8,
    pub rescap: u8,
    pub fpi: u8,
    pub dlfeat: u8,
    pub wawun: u16,
    pub wupf: u16,
    pub npwg: u16,
    pub npwa: u16,
    pub npdg: u16,
    pub npda: u16,
    pub megc: u16,
    pub reserved0: [u8; 80],
    pub eguid: [u8; 16],
    pub nguid: [u8; 16],
    pub reserved1: [u8; 128],
    pub lbaf: [NvmeLbaFormat; 16],
    pub reserved2: [u8; 3712],
}

/// NVMe LBA Format Descriptor Structure
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmeLbaFormat {
    pub ms: u16, // Metadata Size
    pub ds: u8,  // LBA Data Size (2^ds bytes)
    pub rp: u8,  // Relative Performance
}

impl NvmeIdentifyNamespace {
    /// Retrieve sector/block size (in bytes) based on `flbas` index.
    pub fn block_size(&self) -> usize {
        let index = (self.flbas & 0x0F) as usize;
        if index < 16 {
            let ds = self.lbaf[index].ds;
            if ds >= 9 && ds <= 16 {
                return 1usize << ds;
            }
        }
        512 // Default fallback
    }
}

/// Helper functions to construct common NVMe commands
pub struct NvmeCmdBuilder;

impl NvmeCmdBuilder {
    /// Create I/O Completion Queue (Admin Opcode 0x05)
    pub fn create_cq(cid: u16, qid: u16, size: u16, phys_addr: u64) -> NvmeCmd {
        NvmeCmd {
            opcode: NVME_ADMIN_CREATE_CQ,
            flags: 0,
            cid,
            nsid: 0,
            reserved0: 0,
            mptr: 0,
            dptr: [phys_addr, 0],
            cdw10: ((size as u32 - 1) << 16) | (qid as u32),
            cdw11: 0x0001, // Physically contiguous, interrupt disabled
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// Create I/O Submission Queue (Admin Opcode 0x01)
    pub fn create_sq(cid: u16, qid: u16, cq_id: u16, size: u16, phys_addr: u64) -> NvmeCmd {
        NvmeCmd {
            opcode: NVME_ADMIN_CREATE_SQ,
            flags: 0,
            cid,
            nsid: 0,
            reserved0: 0,
            mptr: 0,
            dptr: [phys_addr, 0],
            cdw10: ((size as u32 - 1) << 16) | (qid as u32),
            cdw11: ((cq_id as u32) << 16) | 0x0001, // Associated CQ ID, physically contiguous
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// Identify Namespace (Admin Opcode 0x06)
    pub fn identify_ns(cid: u16, nsid: u32, phys_addr: u64) -> NvmeCmd {
        NvmeCmd {
            opcode: NVME_ADMIN_IDENTIFY,
            flags: 0,
            cid,
            nsid,
            reserved0: 0,
            mptr: 0,
            dptr: [phys_addr, 0],
            cdw10: NVME_IDENTIFY_CNS_NS,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// Read command (NVM I/O Opcode 0x02)
    pub fn read(cid: u16, nsid: u32, lba: u64, block_count: u16, phys_addr: u64) -> NvmeCmd {
        NvmeCmd {
            opcode: NVME_NVM_READ,
            flags: 0,
            cid,
            nsid,
            reserved0: 0,
            mptr: 0,
            dptr: [phys_addr, 0],
            cdw10: (lba & 0xFFFFFFFF) as u32,
            cdw11: ((lba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: (block_count as u32) - 1, // 0-based block count
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// Write command (NVM I/O Opcode 0x01)
    pub fn write(cid: u16, nsid: u32, lba: u64, block_count: u16, phys_addr: u64) -> NvmeCmd {
        NvmeCmd {
            opcode: NVME_NVM_WRITE,
            flags: 0,
            cid,
            nsid,
            reserved0: 0,
            mptr: 0,
            dptr: [phys_addr, 0],
            cdw10: (lba & 0xFFFFFFFF) as u32,
            cdw11: ((lba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: (block_count as u32) - 1, // 0-based block count
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }
}
