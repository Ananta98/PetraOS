pub mod devfs;
pub mod ext2;
pub mod fd;
pub mod ioctl;
pub mod ramfs;
pub mod vfs;

pub use vfs::dentry::Dentry;
pub use vfs::mount::{MOUNT_TABLE, Mount};
pub use vfs::path::{
    create_file, mkdir, readlink, rename, resolve_path, rmdir, stat, symlink, unlink,
};
pub use vfs::types::{
    File, FileOps, FileSystem, Inode, InodeOps, InodeType, O_CREAT, O_RDONLY, O_RDWR, O_WRONLY,
    SeekWhence, Stat, SuperBlock, VfsError, can_read, can_write,
};

/// Run comprehensive POSIX VFS & EXT2 subsystem integration test.
pub fn run_vfs_ext2_tests() {
    log::info!("── Running POSIX VFS & EXT2 Operations Integration Test ──");

    // 1. Test RamFS VFS Operations (Create, Write, Read, Stat, Mkdir, Unlink, Rmdir)
    let ramfs = ramfs::RamFs;
    MOUNT_TABLE
        .lock()
        .mount("/", &ramfs)
        .expect("Failed to mount RamFS root");

    let file_dentry = create_file("/test_posix.txt").expect("create_file failed on VFS root");
    let open_file = File::new(
        file_dentry.clone(),
        O_RDWR,
        file_dentry.inode.ops.open().unwrap(),
    );

    let test_data = b"Hello POSIX VFS & EXT2 System!";
    let written = open_file
        .write(test_data)
        .expect("write failed on VFS file");
    assert_eq!(written, test_data.len());

    open_file.seek(0);
    let mut read_buf = [0u8; 32];
    let read_bytes = open_file
        .read(&mut read_buf)
        .expect("read failed on VFS file");
    assert_eq!(&read_buf[..read_bytes], test_data);

    let st = stat("/test_posix.txt").expect("stat failed on VFS file");
    assert_eq!(st.size, test_data.len() as u64);

    let dir_dentry = mkdir("/test_dir").expect("mkdir failed on VFS root");
    let dir_entries = dir_dentry.inode.ops.readdir().expect("readdir failed");
    assert!(dir_entries.is_empty());

    unlink("/test_posix.txt").expect("unlink failed on VFS file");
    assert!(resolve_path("/test_posix.txt").is_err());

    rmdir("/test_dir").expect("rmdir failed on VFS dir");
    assert!(resolve_path("/test_dir").is_err());

    log::info!("✔ RamFS POSIX operations verified.");

    // Check if real AHCI or NVMe device was detected by PCI initialization
    let device_name = {
        let dm = crate::device::DEVICE_MANAGER.lock();
        let mut target_name = "ext2_mock_disk";
        for dev in dm.get_devices() {
            let dev_lock = dev.lock();
            let name = dev_lock.as_ref().name();
            if name == "AHCI SATA Controller" || name == "NVMe Controller" {
                target_name = name;
                break;
            }
        }
        target_name
    };

    if device_name == "ext2_mock_disk" {
        // Register MockDisk fallback
        let mock_disk = ext2::MockDisk::new(&[], "ext2_mock_disk");
        let dev_arc: alloc::sync::Arc<
            crate::sync::spinlock::Spinlock<alloc::boxed::Box<dyn crate::device::Device>>,
        > = alloc::sync::Arc::new(crate::sync::spinlock::Spinlock::new(
            alloc::boxed::Box::new(mock_disk),
        ));
        crate::device::DEVICE_MANAGER.lock().register(dev_arc);
    } else {
        log::info!(
            "✔ Testing Ext2 directly on real hardware device: '{}'",
            device_name
        );
    }

    // Mount Ext2 filesystem at /mnt using selected device driver.
    // If the device is unformatted, format it dynamically via POSIX ext2::format_ext2 (mkfs.ext2)!
    let ext2_fs = ext2::Ext2Fs::new(device_name);
    mkdir("/mnt").expect("mkdir /mnt failed");
    MOUNT_TABLE
        .lock()
        .mount("/mnt", &ext2_fs)
        .expect("Failed to mount Ext2 at /mnt");

    // Test Ext2 POSIX File Creation and Write
    log::info!("[EXT2 Test] Step 1: Creating file '/mnt/ext2_test.txt'...");
    let ext2_file_dentry = create_file("/mnt/ext2_test.txt").expect("create_file failed on Ext2");
    log::info!(
        "[EXT2 Test]   File created, inode: {}",
        ext2_file_dentry.inode.ino
    );

    let ext2_file = File::new(
        ext2_file_dentry.clone(),
        O_RDWR,
        ext2_file_dentry.inode.ops.open().unwrap(),
    );

    let ext2_msg = b"Ext2 Write POSIX Test Passed!";
    log::info!(
        "[EXT2 Test] Step 2: Writing {} bytes to '/mnt/ext2_test.txt'...",
        ext2_msg.len()
    );
    let written = ext2_file
        .write(ext2_msg)
        .expect("write failed on Ext2 file");
    log::info!("[EXT2 Test]   Successfully wrote {} bytes.", written);

    log::info!("[EXT2 Test] Step 3: Seeking to offset 0...");
    ext2_file.seek(0);

    let mut ext2_read_buf = [0u8; 64];
    log::info!("[EXT2 Test] Step 4: Reading back file data...");
    let n = ext2_file
        .read(&mut ext2_read_buf)
        .expect("read failed on Ext2 file");
    log::info!(
        "[EXT2 Test]   Read back {} bytes: {:?}",
        n,
        core::str::from_utf8(&ext2_read_buf[..n]).unwrap()
    );
    assert_eq!(&ext2_read_buf[..n], ext2_msg);

    // Test Ext2 Directory Creation
    log::info!("[EXT2 Test] Step 5: Creating directory '/mnt/sub_dir'...");
    let ext2_subdir = mkdir("/mnt/sub_dir").expect("mkdir failed on Ext2");
    log::info!(
        "[EXT2 Test]   Directory created, inode: {}",
        ext2_subdir.inode.ino
    );

    log::info!("[EXT2 Test] Step 6: Listing entries in '/mnt/sub_dir'...");
    let readdir_list = ext2_subdir
        .inode
        .ops
        .readdir()
        .expect("readdir on Ext2 subdir failed");
    log::info!(
        "[EXT2 Test]   Readdir found {} entries: {:?}",
        readdir_list.len(),
        readdir_list
    );
    assert_eq!(readdir_list.len(), 2); // '.' and '..'

    // Test Ext2 Stat
    log::info!("[EXT2 Test] Step 7: Querying stat on '/mnt/ext2_test.txt'...");
    let ext2_st = stat("/mnt/ext2_test.txt").expect("stat failed on Ext2 file");
    log::info!(
        "[EXT2 Test]   Stat result: size={}, blocks={}, inode={}",
        ext2_st.size,
        ext2_st.blocks,
        ext2_st.ino
    );
    assert_eq!(ext2_st.size, ext2_msg.len() as u64);

    // Test Ext2 Unlink
    log::info!("[EXT2 Test] Step 8: Unlinking file '/mnt/ext2_test.txt'...");
    unlink("/mnt/ext2_test.txt").expect("unlink failed on Ext2 file");
    assert!(resolve_path("/mnt/ext2_test.txt").is_err());
    log::info!("[EXT2 Test]   Unlink verified: file no longer exists.");

    // Test Ext2 Rmdir
    log::info!("[EXT2 Test] Step 9: Removing directory '/mnt/sub_dir'...");
    rmdir("/mnt/sub_dir").expect("rmdir failed on Ext2 subdir");
    assert!(resolve_path("/mnt/sub_dir").is_err());
    log::info!("[EXT2 Test]   Rmdir verified: directory no longer exists.");

    log::info!(
        "✔ TEST PASSED: POSIX VFS & EXT2 operations (create, write, read, lseek, stat, mkdir, unlink, rmdir) verified successfully!"
    );
}
