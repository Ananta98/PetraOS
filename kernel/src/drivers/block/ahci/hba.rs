/// AHCI HBA (Host Bus Adapter) Controller Operations
///
/// Provides AHCI register constants, controller/port initialization,
/// and port status checking functions.
use ostd::io::IoMem;
use ostd::mm::VmIoOnce;

// ──────────────────────────────────────────────────────────────
// HBA Generic Host Control registers
// ──────────────────────────────────────────────────────────────

/// Global Host Control register offset.
pub const HBA_GHC: usize = 0x04;
/// Ports Implemented register offset.
pub const HBA_PI: usize = 0x0C;

// GHC flag bits
/// AHCI Enable — must be set to use AHCI mode.
pub const GHC_AE: u32 = 1 << 31;
/// HBA Reset — triggers a full controller reset.
pub const GHC_HR: u32 = 1;

// ──────────────────────────────────────────────────────────────
// Port register layout
// ──────────────────────────────────────────────────────────────

/// Base offset of the first port's register block.
pub const PORT_BASE: usize = 0x100;
/// Size of each port's register block.
pub const PORT_SIZE: usize = 0x80;

// Offsets within a port's register block
/// Command List Base Address (lower 32 bits).
pub const PORT_CLB: usize = 0x00;
/// Command List Base Address (upper 32 bits).
pub const PORT_CLBU: usize = 0x04;
/// FIS Base Address (lower 32 bits).
pub const PORT_FB: usize = 0x08;
/// FIS Base Address (upper 32 bits).
pub const PORT_FBU: usize = 0x0C;
/// Interrupt Status.
pub const PORT_IS: usize = 0x10;
/// Command and Status.
pub const PORT_CMD: usize = 0x18;
/// Task File Data.
pub const PORT_TFD: usize = 0x20;
/// Signature — identifies attached device type.
pub const PORT_SIG: usize = 0x24;
/// SATA Status (SCR0: SStatus).
pub const PORT_SSTS: usize = 0x28;
/// Command Issue — one bit per command slot.
pub const PORT_CI: usize = 0x38;

// Command register bits
/// Start — enables command processing.
pub const CMD_ST: u32 = 0x0001;
/// FIS Receive Enable.
pub const CMD_FRE: u32 = 0x0010;
/// Command List Running — indicates DMA engine is active.
pub const CMD_CR: u32 = 0x4000;

// SATA device signatures
/// SATA hard disk signature.
pub const SATA_SIG_DISK: u32 = 0x0000_0101;
/// SATAPI (ATAPI) device signature.
pub const SATA_SIG_ATAPI: u32 = 0xEB14_0101;

// ──────────────────────────────────────────────────────────────
// Helper functions
// ──────────────────────────────────────────────────────────────

/// Compute the base offset for a given port's register block.
#[inline]
pub fn port_offset(port: usize) -> usize {
    PORT_BASE + port * PORT_SIZE
}

/// Initialize the AHCI HBA controller.
///
/// Performs the standard AHCI enable + reset + re-enable sequence:
/// 1. Set AHCI Enable (GHC.AE)
/// 2. Trigger HBA Reset (GHC.HR)
/// 3. Wait for reset to complete (HR bit clears)
/// 4. Re-enable AHCI mode
pub fn init_controller(abar: &IoMem) -> Result<(), ostd::Error> {
    // Enable AHCI mode
    let mut ghc: u32 = abar.read_once(HBA_GHC)?;
    ghc |= GHC_AE;
    abar.write_once(HBA_GHC, &ghc)?;

    // Trigger HBA reset
    ghc = abar.read_once(HBA_GHC)?;
    ghc |= GHC_HR;
    abar.write_once(HBA_GHC, &ghc)?;

    // Wait for reset to complete (HR bit self-clears)
    loop {
        let val: u32 = abar.read_once(HBA_GHC)?;
        if (val & GHC_HR) == 0 {
            break;
        }
    }

    // Re-enable AHCI mode after reset
    ghc = abar.read_once(HBA_GHC)?;
    ghc |= GHC_AE;
    abar.write_once(HBA_GHC, &ghc)?;

    Ok(())
}

/// Initialize a single AHCI port for use.
///
/// Steps:
/// 1. Stop the port (clear ST and FRE, wait for CR to clear)
/// 2. Program the Command List Base and FIS Base addresses
/// 3. Enable FIS Receive, then Start the port
pub fn init_port(
    abar: &IoMem,
    port: usize,
    cmd_list_daddr: u64,
    fis_daddr: u64,
) -> Result<(), ostd::Error> {
    let base = port_offset(port);

    // Stop the port: clear ST and FRE
    let mut cmd: u32 = abar.read_once(base + PORT_CMD)?;
    cmd &= !CMD_ST;
    cmd &= !CMD_FRE;
    abar.write_once(base + PORT_CMD, &cmd)?;

    // Wait for command list to stop running
    loop {
        let val: u32 = abar.read_once(base + PORT_CMD)?;
        if (val & CMD_ST) == 0 && (val & CMD_CR) == 0 {
            break;
        }
    }

    // Set Command List Base Address (64-bit)
    abar.write_once(base + PORT_CLB, &(cmd_list_daddr as u32))?;
    abar.write_once(base + PORT_CLBU, &((cmd_list_daddr >> 32) as u32))?;

    // Set FIS Base Address (64-bit)
    abar.write_once(base + PORT_FB, &(fis_daddr as u32))?;
    abar.write_once(base + PORT_FBU, &((fis_daddr >> 32) as u32))?;

    // Enable FIS Receive
    cmd = abar.read_once(base + PORT_CMD)?;
    cmd |= CMD_FRE;
    abar.write_once(base + PORT_CMD, &cmd)?;

    // Start command processing
    cmd = abar.read_once(base + PORT_CMD)?;
    cmd |= CMD_ST;
    abar.write_once(base + PORT_CMD, &cmd)?;

    Ok(())
}

/// Check if a device is connected and active on a port.
///
/// Returns `true` when Device Detection (DET) == 3 (device present and
/// communication established) and Interface Power Management (IPM) == 1
/// (active state).
pub fn port_connected(abar: &IoMem, port: usize) -> Result<bool, ostd::Error> {
    let ssts: u32 = abar.read_once(port_offset(port) + PORT_SSTS)?;
    let det = ssts & 0x0F;
    let ipm = (ssts >> 8) & 0x0F;
    Ok(det == 3 && ipm == 1)
}

/// Read the device signature from a port.
///
/// Common signatures:
/// - `0x00000101` — SATA disk
/// - `0xEB140101` — SATAPI device
pub fn port_signature(abar: &IoMem, port: usize) -> Result<u32, ostd::Error> {
    abar.read_once(port_offset(port) + PORT_SIG)
}
