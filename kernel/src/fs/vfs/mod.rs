pub mod types;
pub mod dcache;
pub mod mount;
pub mod path;

pub use types::{FileType, Metadata, DirEntry, SeekFrom, InodeOps, FileOps, Dentry, SuperBlock, FileSystem, Result};
pub use mount::{register_filesystem, unregister_filesystem, get_filesystem, init_root_fs, mount, ROOT_DENTRY, CWD_DENTRY};
pub use path::resolve_path;
pub use dcache::DENTRY_CACHE;

#[cfg(ktest)]
mod tests {
    use super::types::{FileType, SeekFrom};
    use super::mount::{register_filesystem, init_root_fs, mount, ROOT_DENTRY};
    use super::path::resolve_path;
    use crate::fs::ramfs::RamFs;
    use alloc::sync::Arc;
    use ostd::prelude::{ktest, println};

    #[ktest]
    fn test_vfs_full() {
        println!("[VFS TEST] Starting test_vfs_full");
        
        // 1. Register and initialize root fs
        let ramfs = Arc::new(RamFs);
        println!("[VFS TEST] Registering ramfs");
        register_filesystem(ramfs.clone()).unwrap();
        println!("[VFS TEST] Initializing root fs");
        init_root_fs("ramfs", &[]).unwrap();

        let root = ROOT_DENTRY.lock().as_ref().cloned().unwrap();
        assert_eq!(root.name, "/");
        println!("[VFS TEST] Root fs initialized successfully");

        // 2. Create directory /etc and file /etc/resolv.conf
        println!("[VFS TEST] Creating /etc");
        let etc_inode = root.inode.mkdir("etc", 0o755).unwrap();
        println!("[VFS TEST] Creating /etc/resolv.conf");
        let file_inode = etc_inode.create("resolv.conf", 0o644).unwrap();
        
        // Write to file
        println!("[VFS TEST] Opening /etc/resolv.conf for writing");
        let mut file_ops = file_inode.open(0).unwrap();
        let mut offset = 0;
        println!("[VFS TEST] Writing to /etc/resolv.conf");
        file_ops.write(b"nameserver 8.8.8.8", &mut offset).unwrap();

        // 3. Resolve path /etc/resolv.conf and read from it
        println!("[VFS TEST] Resolving path /etc/resolv.conf");
        let resolved = resolve_path("/etc/resolv.conf").unwrap();
        assert_eq!(resolved.name, "resolv.conf");
        let meta = resolved.inode.metadata().unwrap();
        assert_eq!(meta.size, 18);
        assert_eq!(meta.file_type, FileType::Regular);

        println!("[VFS TEST] Opening resolved file for reading");
        let mut read_ops = resolved.inode.open(0).unwrap();
        let mut buf = [0u8; 18];
        let mut read_offset = 0;
        println!("[VFS TEST] Reading from resolved file");
        read_ops.read(&mut buf, &mut read_offset).unwrap();
        assert_eq!(&buf, b"nameserver 8.8.8.8");

        // 4. Create directory /mnt and mount another ramfs instance at it
        println!("[VFS TEST] Creating /mnt");
        root.inode.mkdir("mnt", 0o755).unwrap();
        println!("[VFS TEST] Mounting ramfs at /mnt");
        mount("ramfs", "/mnt", 0, &[]).unwrap();

        // Resolve /mnt -> should be the root of the mounted filesystem (named "/")
        println!("[VFS TEST] Resolving path /mnt");
        let mnt_resolved = resolve_path("/mnt").unwrap();
        assert_eq!(mnt_resolved.name, "/");

        // Create a file in /mnt/test.txt
        println!("[VFS TEST] Creating /mnt/test.txt");
        let test_inode = mnt_resolved.inode.create("test.txt", 0o644).unwrap();
        println!("[VFS TEST] Opening /mnt/test.txt for writing");
        let mut test_ops = test_inode.open(0).unwrap();
        let mut test_offset = 0;
        println!("[VFS TEST] Writing to /mnt/test.txt");
        test_ops.write(b"hello mountpoint", &mut test_offset).unwrap();

        // Resolve /mnt/test.txt
        println!("[VFS TEST] Resolving path /mnt/test.txt");
        let test_resolved = resolve_path("/mnt/test.txt").unwrap();
        assert_eq!(test_resolved.name, "test.txt");

        // 5. Test mount point crossing back ".." -> /mnt/test.txt/../.. -> ROOT
        println!("[VFS TEST] Resolving path /mnt/test.txt/../..");
        let cross_resolved = resolve_path("/mnt/test.txt/../..").unwrap();
        assert_eq!(cross_resolved.name, "/");

        // 6. Test relative symlinks: /etc/hosts -> resolv.conf
        println!("[VFS TEST] Creating relative symlink /etc/hosts -> resolv.conf");
        etc_inode.symlink("hosts", "resolv.conf").unwrap();
        println!("[VFS TEST] Resolving /etc/hosts");
        let sym_resolved = resolve_path("/etc/hosts").unwrap();
        assert_eq!(sym_resolved.name, "resolv.conf");

        // Test absolute symlinks: /etc/hosts_abs -> /etc/resolv.conf
        println!("[VFS TEST] Creating absolute symlink /etc/hosts_abs -> /etc/resolv.conf");
        etc_inode.symlink("hosts_abs", "/etc/resolv.conf").unwrap();
        println!("[VFS TEST] Resolving /etc/hosts_abs");
        let sym_abs_resolved = resolve_path("/etc/hosts_abs").unwrap();
        assert_eq!(sym_abs_resolved.name, "resolv.conf");
        
        println!("[VFS TEST] All VFS tests passed successfully!");
    }
}
