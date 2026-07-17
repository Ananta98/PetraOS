pub mod dcache;
pub mod mount;
pub mod path;
pub mod types;

pub use dcache::DENTRY_CACHE;
pub use mount::{
    CWD_DENTRY, ROOT_DENTRY, get_filesystem, init_root_fs, mount, register_filesystem,
    unregister_filesystem,
};
pub use path::resolve_path;
pub use types::{
    Dentry, DirEntry, FileOps, FileSystem, FileType, InodeOps, Metadata, Result, SeekFrom,
    SuperBlock,
};

#[cfg(ktest)]
mod tests {
    use super::mount::{ROOT_DENTRY, init_root_fs, mount, register_filesystem};
    use super::path::resolve_path;
    use super::types::{FileType, SeekFrom};
    use crate::fs::ramfs::RamFs;
    use alloc::sync::Arc;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_vfs_full() {
        // 1. Register and initialize root fs
        let ramfs = Arc::new(RamFs);
        register_filesystem(ramfs.clone()).unwrap();
        init_root_fs("ramfs", &[]).unwrap();

        let root = ROOT_DENTRY.lock().as_ref().cloned().unwrap();
        assert_eq!(root.name, "/");

        // 2. Create directory /etc and file /etc/resolv.conf
        let etc_inode = root.inode.mkdir("etc", 0o755).unwrap();
        let file_inode = etc_inode.create("resolv.conf", 0o644).unwrap();

        // Write to file
        let mut file_ops = file_inode.open(0).unwrap();
        let mut offset = 0;
        file_ops.write(b"nameserver 8.8.8.8", &mut offset).unwrap();

        // 3. Resolve path /etc/resolv.conf and read from it
        let resolved = resolve_path("/etc/resolv.conf").unwrap();
        assert_eq!(resolved.name, "resolv.conf");
        let meta = resolved.inode.metadata().unwrap();
        assert_eq!(meta.size, 18);
        assert_eq!(meta.file_type, FileType::Regular);

        let mut read_ops = resolved.inode.open(0).unwrap();
        let mut buf = [0u8; 18];
        let mut read_offset = 0;
        read_ops.read(&mut buf, &mut read_offset).unwrap();
        assert_eq!(&buf, b"nameserver 8.8.8.8");

        // 4. Create directory /mnt and mount another ramfs instance at it
        root.inode.mkdir("mnt", 0o755).unwrap();
        mount("ramfs", "/mnt", 0, &[]).unwrap();

        // Resolve /mnt -> should be the root of the mounted filesystem (named "/")
        let mnt_resolved = resolve_path("/mnt").unwrap();
        assert_eq!(mnt_resolved.name, "/");

        // Create a file in /mnt/test.txt
        let test_inode = mnt_resolved.inode.create("test.txt", 0o644).unwrap();
        let mut test_ops = test_inode.open(0).unwrap();
        let mut test_offset = 0;
        test_ops
            .write(b"hello mountpoint", &mut test_offset)
            .unwrap();

        // Resolve /mnt/test.txt
        let test_resolved = resolve_path("/mnt/test.txt").unwrap();
        assert_eq!(test_resolved.name, "test.txt");

        // 5. Test mount point crossing back ".." -> /mnt/test.txt/../.. -> ROOT
        let cross_resolved = resolve_path("/mnt/test.txt/../..").unwrap();
        assert_eq!(cross_resolved.name, "/");

        // 6. Test relative symlinks: /etc/hosts -> resolv.conf
        etc_inode.symlink("hosts", "resolv.conf").unwrap();
        let sym_resolved = resolve_path("/etc/hosts").unwrap();
        assert_eq!(sym_resolved.name, "resolv.conf");

        // Test absolute symlinks: /etc/hosts_abs -> /etc/resolv.conf
        etc_inode.symlink("hosts_abs", "/etc/resolv.conf").unwrap();
        let sym_abs_resolved = resolve_path("/etc/hosts_abs").unwrap();
        assert_eq!(sym_abs_resolved.name, "resolv.conf");
    }
}
