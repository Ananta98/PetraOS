use crate::mm::PageFaultAccess;

/// Architecture-specific x86_64 Page Fault Error Code representation.
///
/// Ref: Intel SDM Vol. 3A Section 4.7 & AMD64 Architecture Programmer's Manual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchPageFaultErrorCode(pub u64);

impl ArchPageFaultErrorCode {
    pub const PRESENT: u64 = 1 << 0; // Bit 0: 0 = not present, 1 = protection violation
    pub const WRITE: u64 = 1 << 1; // Bit 1: 0 = read, 1 = write
    pub const USER: u64 = 1 << 2; // Bit 2: 0 = supervisor, 1 = user
    pub const RESERVED_WRITE: u64 = 1 << 3; // Bit 3: 1 = reserved bit violation
    pub const INSTRUCTION_FETCH: u64 = 1 << 4; // Bit 4: 1 = instruction fetch (NX violation)
    pub const PK: u64 = 1 << 5; // Bit 5: Protection key violation
    pub const SS: u64 = 1 << 6; // Bit 6: Shadow stack access

    pub fn from_raw(code: u64) -> Self {
        Self(code)
    }

    /// Is fault caused by a protection violation (page is present)?
    pub fn is_protection_violation(&self) -> bool {
        (self.0 & Self::PRESENT) != 0
    }

    /// Is access a write operation?
    pub fn is_write(&self) -> bool {
        (self.0 & Self::WRITE) != 0
    }

    /// Is access executed from User mode (CPL=3)?
    pub fn is_user(&self) -> bool {
        (self.0 & Self::USER) != 0
    }

    /// Is access an instruction fetch?
    pub fn is_instruction_fetch(&self) -> bool {
        (self.0 & Self::INSTRUCTION_FETCH) != 0
    }

    /// Convert x86_64 hardware error code into generic architecture-independent `PageFaultAccess`.
    pub fn to_generic_access(&self) -> PageFaultAccess {
        let mut access = PageFaultAccess::empty();
        if self.is_protection_violation() {
            access |= PageFaultAccess::PRESENT;
        }
        if self.is_write() {
            access |= PageFaultAccess::WRITE;
        }
        if self.is_user() {
            access |= PageFaultAccess::USER;
        }
        if self.is_instruction_fetch() {
            access |= PageFaultAccess::EXECUTE;
        }
        access
    }
}
