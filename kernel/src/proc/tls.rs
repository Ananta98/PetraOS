use crate::vm::vma::VmaManager;
use alloc::vec::Vec;
use ostd::Error;
use ostd::mm::{PAGE_SIZE, PageFlags, Vaddr};

/// Per-process template for initializing per-thread TLS blocks
/// from an ELF's `PT_TLS` program header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsTemplate {
    /// Raw template data: `.tdata` content followed by zero padding
    /// up to `mem_size` bytes (`.tdata` + `.tbss`).
    pub data: Vec<u8>,
    /// Alignment requirement from the `PT_TLS` segment.
    pub align: usize,
}

impl TlsTemplate {
    /// Build a template from the raw ELF `PT_TLS` parameters.
    pub fn new(tdata: &[u8], mem_size: usize, align: usize) -> Self {
        let mut data = alloc::vec![0u8; mem_size];
        data[..tdata.len()].copy_from_slice(tdata);
        Self { data, align }
    }

    /// Total size of the TLS block (`.tdata` + `.tbss`).
    pub fn mem_size(&self) -> usize {
        self.data.len()
    }

    /// `true` when there is no TLS segment (no thread-local storage).
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl Default for TlsTemplate {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            align: 1,
        }
    }
}

/// Allocate a TLS block in the process's user VM and return the address
/// that the **FS base** should be set to.
///
/// On x86‑64 the FS segment base (the thread pointer) conventionally
/// points to the *end* of the per-thread static TLS block, so that
/// TLS variables can be accessed at negative offsets from the segment
/// base (x86‑64 TLS Variant II / `TLS_DTV_AT_TP` model).
///
/// Returns `0` if the template is empty (no TLS needed).
pub fn allocate_tls_block(vm: &VmaManager, tpl: &TlsTemplate) -> Result<Vaddr, Error> {
    if tpl.is_empty() {
        return Ok(0);
    }

    let size = align_up(tpl.mem_size(), PAGE_SIZE);
    let addr = vm.find_free_region(size).ok_or(Error::NoMemory)?;
    vm.map_region(addr, size, PageFlags::RW)?;
    vm.copy_to_user(addr, &tpl.data)?;

    // x86-64 ABI: FS base points to the byte just past the static TLS block.
    Ok(addr + tpl.mem_size())
}

/// Write `addr` into the FS-base MSR on the **current CPU** using
/// the `wrfsbase` instruction.
///
/// The FSGSBASE CPU feature is enabled by the OSTD boot code, so this
/// instruction is available at privilege level 0.
pub fn set_fs_base(addr: Vaddr) {
    use ostd::arch::cpu::context::FsBase;
    FsBase::new(addr).load();
}

/// Read the current CPU's FS-base MSR using the `rdfsbase` instruction.
pub fn get_fs_base() -> Vaddr {
    use ostd::arch::cpu::context::FsBase;
    let mut fs = FsBase::default();
    fs.save();
    fs.addr()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
