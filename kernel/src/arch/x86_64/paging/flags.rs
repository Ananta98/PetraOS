use x86_64::registers::model_specific::{Efer, EferFlags};

/// Enable the No-Execute (NXE) bit in EFER MSR.
pub unsafe fn enable_nxe() {
    let mut efer = Efer::read();
    efer.insert(EferFlags::NO_EXECUTE_ENABLE);
    // SAFETY: Enabling NXE bit in EFER is standard for x86_64 paging.
    unsafe {
        Efer::write(efer);
    }
}
