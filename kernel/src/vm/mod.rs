pub mod fault;
pub mod fork;
pub mod protect;
pub mod region;
pub mod vma;

use crate::vm::vma::VmaManager;
use alloc::sync::Arc;
use fault::handle_page_fault;
use ostd::arch::trap::inject_user_page_fault_handler;
pub use region::MmapFileBacking;
use spin::Once;

pub static VMA_MANAGER: Once<Arc<VmaManager>> = Once::new();

pub fn init() {
    let manager = Arc::new(VmaManager::new());
    manager.activate();

    VMA_MANAGER.call_once(|| manager);

    inject_user_page_fault_handler(handle_page_fault);
}
