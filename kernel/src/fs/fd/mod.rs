pub mod fd_table;

use crate::fs::flags::{O_RDONLY, O_RDWR};
use crate::fs::path::resolve_path;
use crate::fs::vfs::file::File;
use crate::fs::vfs::mount::MOUNT_TABLE;
use alloc::sync::Arc;
pub use fd_table::FdTable;

/// Bind standard input/output/error descriptors (0, 1, 2) for the init process.
pub fn setup_init_std_fds() {
    let console_dentry = resolve_path("/dev/console").expect("Failed to resolve /dev/console");
    let console_ops = console_dentry
        .inode
        .ops
        .open()
        .expect("Failed to open /dev/console");
    let console_file = Arc::new(File::new(console_dentry, O_RDWR, console_ops));

    let mut pm = crate::proc::PROCESS_MANAGER.lock();
    if let Some(init_proc) = pm.get_process_mut(crate::proc::ProcessId(1)) {
        init_proc.setup_std_fds(console_file);
    }
}
