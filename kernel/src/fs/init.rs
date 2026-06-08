use alloc::sync::Arc;
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::dentry::Dentry;
use crate::fs::vfs::file::File;
use crate::fs::ramfs::RamFs;

use crate::fs::devfs::DevFs;
use crate::fs::devfs::console::ConsoleInode;
use crate::fs::tmpfs::TmpFs;
use crate::fs::procfs::ProcFs;
use crate::fs::flags::O_RDWR;
use crate::fs::path::resolve_path;

/// Initialize the Virtual File System.
///
/// Mounts the following filesystems:
/// - ramfs at `/` (root)
/// - devfs at `/dev` (with `/dev/console`)
/// - tmpfs at `/tmp`
/// - procfs at `/proc`
///
/// Sets up stdin/stdout/stderr (FDs 0, 1, 2) for the init process (PID 1)
/// bound to `/dev/console`.
pub fn init() {
    // 1. Mount ramfs at "/"
    {
        let mut mt = MOUNT_TABLE.lock();
        mt.mount("/", &RamFs).expect("Failed to mount ramfs at /");
    }
    log::info!("Mounted ramfs at /");

    // 2. Create mountpoint directories on the root filesystem
    {
        let root_dentry = {
            let mt = MOUNT_TABLE.lock();
            mt.root().expect("No root mount").root_dentry.clone()
        };
        root_dentry.inode.ops.mkdir("dev").expect("Failed to create /dev");
        Dentry::add_child(&root_dentry, "dev".into(),
            root_dentry.inode.ops.lookup("dev").expect("Failed to lookup /dev"));

        root_dentry.inode.ops.mkdir("tmp").expect("Failed to create /tmp");
        Dentry::add_child(&root_dentry, "tmp".into(),
            root_dentry.inode.ops.lookup("tmp").expect("Failed to lookup /tmp"));

        root_dentry.inode.ops.mkdir("proc").expect("Failed to create /proc");
        Dentry::add_child(&root_dentry, "proc".into(),
            root_dentry.inode.ops.lookup("proc").expect("Failed to lookup /proc"));
    }

    // 3. Mount devfs at "/dev"
    {
        let mut mt = MOUNT_TABLE.lock();
        let dev_mount = mt.mount("/dev", &DevFs).expect("Failed to mount devfs at /dev");

        // Register console device
        let console_ino = dev_mount.superblock.alloc_ino();
        let console_inode = Arc::new(crate::fs::vfs::inode::Inode {
            ino: console_ino,
            inode_type: crate::fs::vfs::inode::InodeType::CharDevice,
            ops: Arc::new(ConsoleInode),
        });

        // Add to the devfs root dentry so path resolution finds it.
        Dentry::add_child(&dev_mount.root_dentry, "console".into(), console_inode);
    }
    log::info!("Mounted devfs at /dev with console device.");

    // 4. Mount tmpfs at "/tmp"
    {
        let mut mt = MOUNT_TABLE.lock();
        mt.mount("/tmp", &TmpFs).expect("Failed to mount tmpfs at /tmp");
    }
    log::info!("Mounted tmpfs at /tmp");

    // 5. Mount procfs at "/proc"
    {
        let mut mt = MOUNT_TABLE.lock();
        mt.mount("/proc", &ProcFs).expect("Failed to mount procfs at /proc");
    }
    log::info!("Mounted procfs at /proc");

    log::info!("Virtual File System (VFS) initialized.");

    // 6. Set up standard FDs (0, 1, 2) for the init process (PID 1)
    let console_dentry = resolve_path("/dev/console")
        .expect("Failed to resolve /dev/console");
    let console_ops = console_dentry.inode.ops.open()
        .expect("Failed to open /dev/console");
    let console_file = Arc::new(File::new(console_dentry, O_RDWR, console_ops));

    let mut pm = crate::proc::PROCESS_MANAGER.lock();
    if let Some(init_proc) = pm.get_process_mut(crate::proc::ProcessId(1)) {
        init_proc.setup_std_fds(console_file);
        log::info!("Standard FDs (0, 1, 2) bound to /dev/console for init process.");
    }
}
