use crate::drivers::timer::Timer;
use crate::drivers::timer::Tsc;
/// Process filesystem (`procfs`) for PetraOS.
///
/// Exposes kernel process state as a synthetic read-only filesystem,
/// mirroring the layout of Linux's `/proc`:
///
/// ```text
/// /proc/
///   version          — kernel version string
///   uptime           — seconds.centiseconds since boot (TSC-based)
///   self             — symlink to /proc/<current-pid>
///   <pid>/
///     status         — human-readable process attributes (name, pid, ppid, state, uids, gids)
///     stat           — machine-readable single-line stat (Linux compat)
///     cmdline        — process name as a null-terminated string
///     fd/            — directory of open file descriptor numbers
/// ```
///
/// # Design
///
/// ProcFS is a **virtual** filesystem: no data is stored on disk.  Every
/// `read` call derives its content live from [`PROCESS_TABLE`] and the
/// TSC timer.  The implementation follows the same [`InodeOps`] /
/// [`FileOps`] interface used by `devfs` and `ramfs`.
///
/// Inodes fall into four kinds:
///
/// | Kind | Rust type | VFS file type |
/// |------|-----------|---------------|
/// | Root `/proc` directory | [`ProcRootInode`] | `Directory` |
/// | Per-PID directory `/proc/<pid>` | [`ProcPidInode`] | `Directory` |
/// | Per-PID FD directory `/proc/<pid>/fd` | [`ProcFdDirInode`] | `Directory` |
/// | Read-only text file | [`ProcFileInode`] | `Regular` |
/// | Symlink (`self`) | [`ProcSymlinkInode`] | `Symlink` |
use crate::fs::vfs::{
    Dentry, DirEntry, FileOps, FileSystem, FileType, InodeOps, Metadata, Result, SeekFrom,
    SuperBlock,
};
use crate::proc::pid_table::{PROCESS_TABLE, Pid};
use crate::proc::process::{Process, ProcessState};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use ostd::Error;
use ostd::sync::SpinLock;
use spin::Once;

// ---------------------------------------------------------------------------
// Inode number allocator
// ---------------------------------------------------------------------------

static INODE_COUNTER: AtomicU64 = AtomicU64::new(2000);

fn next_inode() -> u64 {
    INODE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format `ProcessState` as a Linux-compat single letter.
fn state_char(state: ProcessState) -> char {
    match state {
        ProcessState::Ready | ProcessState::Running => 'R',
        ProcessState::Sleeping => 'S',
        ProcessState::Zombie => 'Z',
    }
}

/// Render a process `status` file, matching the Linux `/proc/<pid>/status`
/// format for the fields that PetraOS tracks.
fn format_status(proc: &Process) -> String {
    format!(
        "Name:\t{name}\n\
         State:\t{state} ({state_name})\n\
         Pid:\t{pid}\n\
         PPid:\t{ppid}\n\
         Uid:\t{uid}\t{euid}\t{suid}\t{fsuid}\n\
         Gid:\t{gid}\t{egid}\t{sgid}\t{fsgid}\n",
        name = proc.name,
        state = state_char(proc.state),
        state_name = match proc.state {
            ProcessState::Ready => "runnable",
            ProcessState::Running => "running",
            ProcessState::Sleeping => "sleeping",
            ProcessState::Zombie => "zombie",
        },
        pid = proc.pid.as_u32(),
        ppid = proc.ppid.map_or(0, |p| p.as_u32()),
        uid = proc.uid,
        euid = proc.euid,
        suid = proc.suid,
        fsuid = proc.fsuid,
        gid = proc.gid,
        egid = proc.egid,
        sgid = proc.sgid,
        fsgid = proc.fsgid,
    )
}

/// Render a process `stat` file — a single space-separated line matching the
/// Linux `/proc/<pid>/stat` field order for the fields PetraOS tracks.
/// Fields not tracked here are emitted as `0`.
fn format_stat(proc: &Process) -> String {
    format!(
        "{pid} ({name}) {state} {ppid} {pgid} {sid} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
        pid = proc.pid.as_u32(),
        name = proc.name,
        state = state_char(proc.state),
        ppid = proc.ppid.map_or(0, |p| p.as_u32()),
        pgid = proc.pgid.as_u32(),
        sid = proc.sid.as_u32(),
    )
}

/// Render a process `cmdline` file — process name followed by a null byte,
/// matching the Linux convention.
fn format_cmdline(proc: &Process) -> String {
    let mut s = proc.name.clone();
    s.push('\0');
    s
}

/// Read the current TSC-based monotonic time in milliseconds.
fn uptime_ms() -> u64 {
    Tsc::new().current_time_ns() / 1_000_000
}

/// Format the `uptime` file: `<seconds>.<centiseconds>\n`.
fn format_uptime() -> String {
    let ms = uptime_ms();
    let secs = ms / 1000;
    let centis = (ms % 1000) / 10;
    format!("{secs}.{centis:02} 0.00\n")
}

/// Format the `version` file.
fn format_version() -> String {
    String::from("PetraOS version 0.1.0 (Rust kernel) #1\n")
}

// ---------------------------------------------------------------------------
// ProcFileInode — a read-only in-memory text file
//
// Content is generated lazily on every `open()` by a caller-supplied closure
// returning a `String`.  This avoids caching stale kernel state.
// ---------------------------------------------------------------------------

/// A virtual read-only text file whose content is produced on demand.
///
/// `content_fn` is called once per `open()` to snapshot the relevant kernel
/// state into a `String`.
pub struct ProcFileInode {
    inode_num: u64,
    /// Generator closure — called every time the file is opened.
    content_fn: Arc<dyn Fn() -> String + Send + Sync>,
}

impl ProcFileInode {
    fn new(content_fn: impl Fn() -> String + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            inode_num: next_inode(),
            content_fn: Arc::new(content_fn),
        })
    }
}

impl InodeOps for ProcFileInode {
    fn lookup(&self, _name: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn metadata(&self) -> Result<Metadata> {
        let content = (self.content_fn)();
        Ok(Metadata {
            size: content.len(),
            file_type: FileType::Regular,
            mode: 0o444,
            inode_num: self.inode_num,
            nlink: 1,
        })
    }
    fn read_link(&self) -> Result<String> {
        Err(Error::InvalidArgs)
    }
    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        let content = (self.content_fn)();
        Ok(Box::new(ProcFile {
            content: content.into_bytes(),
        }))
    }
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn unlink(&self, _name: &str) -> Result<()> {
        Err(Error::InvalidArgs)
    }
    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<()> {
        Err(Error::InvalidArgs)
    }
}

/// Open-file handle for a [`ProcFileInode`].
struct ProcFile {
    content: Vec<u8>,
}

impl FileOps for ProcFile {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize> {
        if *offset >= self.content.len() {
            return Ok(0);
        }
        let available = &self.content[*offset..];
        let count = core::cmp::min(buf.len(), available.len());
        buf[..count].copy_from_slice(&available[..count]);
        *offset += count;
        Ok(count)
    }
    fn write(&mut self, _buf: &[u8], _offset: &mut usize) -> Result<usize> {
        Err(Error::InvalidArgs)
    }
    fn seek(&mut self, pos: SeekFrom, offset: &mut usize) -> Result<usize> {
        let len = self.content.len();
        let new_offset = match pos {
            SeekFrom::Start(n) => n as isize,
            SeekFrom::Current(n) => *offset as isize + n,
            SeekFrom::End(n) => len as isize + n,
        };
        if new_offset < 0 || new_offset as usize > len {
            return Err(Error::InvalidArgs);
        }
        *offset = new_offset as usize;
        Ok(*offset)
    }
    fn readdir(&mut self) -> Result<Vec<DirEntry>> {
        Err(Error::InvalidArgs)
    }
}

// ---------------------------------------------------------------------------
// ProcSymlinkInode — a read-only symlink
// ---------------------------------------------------------------------------

/// A virtual symlink (used for `/proc/self`).
struct ProcSymlinkInode {
    inode_num: u64,
    /// Closure that produces the symlink target on each `read_link()` call.
    target_fn: Arc<dyn Fn() -> String + Send + Sync>,
}

impl ProcSymlinkInode {
    fn new(target_fn: impl Fn() -> String + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            inode_num: next_inode(),
            target_fn: Arc::new(target_fn),
        })
    }
}

impl InodeOps for ProcSymlinkInode {
    fn lookup(&self, _name: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn metadata(&self) -> Result<Metadata> {
        let target = (self.target_fn)();
        Ok(Metadata {
            size: target.len(),
            file_type: FileType::Symlink,
            mode: 0o777,
            inode_num: self.inode_num,
            nlink: 1,
        })
    }
    fn read_link(&self) -> Result<String> {
        Ok((self.target_fn)())
    }
    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        Err(Error::InvalidArgs)
    }
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn unlink(&self, _name: &str) -> Result<()> {
        Err(Error::InvalidArgs)
    }
    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<()> {
        Err(Error::InvalidArgs)
    }
}

// ---------------------------------------------------------------------------
// ProcFdDirInode — /proc/<pid>/fd/
// ---------------------------------------------------------------------------

/// Virtual directory listing the open file descriptors of a process.
///
/// Each entry is a symlink named by the fd number, pointing to the
/// abstract path `pipe:[N]`, `socket:[N]`, or `anon_inode:N` as
/// appropriate.  For now all fds resolve to `anon_inode:<fd>`.
struct ProcFdDirInode {
    inode_num: u64,
    pid: Pid,
}

impl ProcFdDirInode {
    fn new(pid: Pid) -> Arc<Self> {
        Arc::new(Self {
            inode_num: next_inode(),
            pid,
        })
    }

    /// Collect the open fd numbers for this PID.
    fn fd_list(&self) -> Vec<i32> {
        PROCESS_TABLE
            .get_process(self.pid)
            .map(|proc| proc.fd_table.lock().list_fds())
            .unwrap_or_default()
    }
}

impl InodeOps for ProcFdDirInode {
    fn lookup(&self, name: &str) -> Result<Arc<dyn InodeOps>> {
        let fd: i32 = name.parse().map_err(|_| Error::InvalidArgs)?;
        let fds = self.fd_list();
        if fds.contains(&fd) {
            // Symlink pointing to an abstract fd path.
            let target = format!("anon_inode:{fd}");
            Ok(ProcSymlinkInode::new(move || target.clone()))
        } else {
            Err(Error::InvalidArgs)
        }
    }
    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn metadata(&self) -> Result<Metadata> {
        Ok(Metadata {
            size: 0,
            file_type: FileType::Directory,
            mode: 0o500,
            inode_num: self.inode_num,
            nlink: 1,
        })
    }
    fn read_link(&self) -> Result<String> {
        Err(Error::InvalidArgs)
    }
    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        let fds = self.fd_list();
        let inode_num = self.inode_num;
        Ok(Box::new(ProcDirFile {
            entries: {
                let mut v = Vec::new();
                v.push(DirEntry {
                    name: String::from("."),
                    inode_num,
                    file_type: FileType::Directory,
                });
                v.push(DirEntry {
                    name: String::from(".."),
                    inode_num: 0,
                    file_type: FileType::Directory,
                });
                for fd in fds {
                    v.push(DirEntry {
                        name: fd.to_string(),
                        inode_num: next_inode(),
                        file_type: FileType::Symlink,
                    });
                }
                v
            },
        }))
    }
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn unlink(&self, _name: &str) -> Result<()> {
        Err(Error::InvalidArgs)
    }
    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<()> {
        Err(Error::InvalidArgs)
    }
}

// ---------------------------------------------------------------------------
// ProcPidInode — /proc/<pid>/
// ---------------------------------------------------------------------------

/// Virtual directory for a single process, containing `status`, `stat`,
/// `cmdline`, and `fd/`.
pub struct ProcPidInode {
    inode_num: u64,
    pid: Pid,
}

impl ProcPidInode {
    fn new(pid: Pid) -> Arc<Self> {
        Arc::new(Self {
            inode_num: next_inode(),
            pid,
        })
    }
}

impl InodeOps for ProcPidInode {
    fn lookup(&self, name: &str) -> Result<Arc<dyn InodeOps>> {
        let pid = self.pid;
        let proc = PROCESS_TABLE.get_process(pid).ok_or(Error::InvalidArgs)?;

        match name {
            "status" => Ok(ProcFileInode::new(move || {
                PROCESS_TABLE
                    .get_process(pid)
                    .map(|p| format_status(&p))
                    .unwrap_or_default()
            })),
            "stat" => Ok(ProcFileInode::new(move || {
                PROCESS_TABLE
                    .get_process(pid)
                    .map(|p| format_stat(&p))
                    .unwrap_or_default()
            })),
            "cmdline" => {
                // Capture the name at lookup time — cmdline is effectively
                // static unless execve overwrites it, at which point the
                // dentry cache will expire naturally.
                let name_snapshot = proc.name.clone();
                Ok(ProcFileInode::new(move || {
                    let mut s = name_snapshot.clone();
                    s.push('\0');
                    s
                }))
            }
            "fd" => Ok(ProcFdDirInode::new(pid)),
            _ => Err(Error::InvalidArgs),
        }
    }
    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn metadata(&self) -> Result<Metadata> {
        // Return ENOENT if the process has already been reaped.
        PROCESS_TABLE
            .get_process(self.pid)
            .ok_or(Error::InvalidArgs)?;
        Ok(Metadata {
            size: 0,
            file_type: FileType::Directory,
            mode: 0o555,
            inode_num: self.inode_num,
            nlink: 1,
        })
    }
    fn read_link(&self) -> Result<String> {
        Err(Error::InvalidArgs)
    }
    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        let pid_u32 = self.pid.as_u32();
        let inode_num = self.inode_num;
        Ok(Box::new(ProcDirFile {
            entries: {
                let mut v = Vec::new();
                v.push(DirEntry {
                    name: String::from("."),
                    inode_num,
                    file_type: FileType::Directory,
                });
                v.push(DirEntry {
                    name: String::from(".."),
                    inode_num: 0,
                    file_type: FileType::Directory,
                });
                for entry_name in &["status", "stat", "cmdline", "fd"] {
                    v.push(DirEntry {
                        name: String::from(*entry_name),
                        inode_num: next_inode(),
                        file_type: if *entry_name == "fd" {
                            FileType::Directory
                        } else {
                            FileType::Regular
                        },
                    });
                }
                let _ = pid_u32; // suppress unused warning
                v
            },
        }))
    }
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn unlink(&self, _name: &str) -> Result<()> {
        Err(Error::InvalidArgs)
    }
    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<()> {
        Err(Error::InvalidArgs)
    }
}

// ---------------------------------------------------------------------------
// ProcRootInode — /proc/
// ---------------------------------------------------------------------------

/// Root inode for the entire procfs mount.
///
/// Dynamically enumerates the [`PROCESS_TABLE`] on every `open()` /
/// `lookup()` call so that new processes and exited processes are always
/// reflected correctly without any persistent state.
pub struct ProcRootInode {
    inode_num: u64,
}

impl ProcRootInode {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inode_num: next_inode(),
        })
    }

    /// Collect all living PIDs from the global process table.
    fn all_pids() -> Vec<Pid> {
        // PROCESS_TABLE does not expose an iterator, so we probe a
        // reasonable range.  A proper implementation would add a
        // `list_pids()` method to ProcessTable; for now we rely on PIDs
        // being u32 values allocated from 1 upward.
        //
        // We call get_process for consecutive values starting at 1 and
        // stop after 4096 misses in a row (PID space is sparse but bounded).
        let mut pids = Vec::new();
        let mut consecutive_misses: u32 = 0;
        let mut candidate: u32 = 1;

        while consecutive_misses < 64 && candidate < 65536 {
            let pid = Pid::from_raw(candidate);
            if PROCESS_TABLE.get_process(pid).is_some() {
                pids.push(pid);
                consecutive_misses = 0;
            } else {
                consecutive_misses += 1;
            }
            candidate += 1;
        }
        pids
    }
}

impl InodeOps for ProcRootInode {
    fn lookup(&self, name: &str) -> Result<Arc<dyn InodeOps>> {
        match name {
            "version" => Ok(ProcFileInode::new(format_version)),
            "uptime" => Ok(ProcFileInode::new(format_uptime)),
            "self" => {
                // `/proc/self` is a symlink to the current process's PID dir.
                Ok(ProcSymlinkInode::new(|| {
                    let pid = Process::current().pid.as_u32();
                    format!("/proc/{pid}")
                }))
            }
            _ => {
                // Attempt to parse as a PID number.
                let pid_num: u32 = name.parse().map_err(|_| Error::InvalidArgs)?;
                let pid = Pid::from_raw(pid_num);
                PROCESS_TABLE.get_process(pid).ok_or(Error::InvalidArgs)?;
                Ok(ProcPidInode::new(pid))
            }
        }
    }
    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }
    fn metadata(&self) -> Result<Metadata> {
        Ok(Metadata {
            size: 0,
            file_type: FileType::Directory,
            mode: 0o555,
            inode_num: self.inode_num,
            nlink: 1,
        })
    }
    fn read_link(&self) -> Result<String> {
        Err(Error::InvalidArgs)
    }
    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        let inode_num = self.inode_num;
        let pids = Self::all_pids();

        let mut entries = Vec::new();
        entries.push(DirEntry {
            name: String::from("."),
            inode_num,
            file_type: FileType::Directory,
        });
        entries.push(DirEntry {
            name: String::from(".."),
            inode_num: 0,
            file_type: FileType::Directory,
        });
        // Global pseudo-files
        for &fname in &["version", "uptime"] {
            entries.push(DirEntry {
                name: String::from(fname),
                inode_num: next_inode(),
                file_type: FileType::Regular,
            });
        }
        entries.push(DirEntry {
            name: String::from("self"),
            inode_num: next_inode(),
            file_type: FileType::Symlink,
        });
        // Per-PID entries
        for pid in pids {
            entries.push(DirEntry {
                name: pid.as_u32().to_string(),
                inode_num: next_inode(),
                file_type: FileType::Directory,
            });
        }

        Ok(Box::new(ProcDirFile { entries }))
    }
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn unlink(&self, _name: &str) -> Result<()> {
        Err(Error::InvalidArgs)
    }
    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<()> {
        Err(Error::InvalidArgs)
    }
}

// ---------------------------------------------------------------------------
// ProcDirFile — generic directory file-ops handle
// ---------------------------------------------------------------------------

/// A snapshotted directory listing returned by any procfs `open()` on a
/// directory inode.
struct ProcDirFile {
    entries: Vec<DirEntry>,
}

impl FileOps for ProcDirFile {
    fn read(&mut self, _buf: &mut [u8], _offset: &mut usize) -> Result<usize> {
        Err(Error::InvalidArgs)
    }
    fn write(&mut self, _buf: &[u8], _offset: &mut usize) -> Result<usize> {
        Err(Error::InvalidArgs)
    }
    fn seek(&mut self, _pos: SeekFrom, offset: &mut usize) -> Result<usize> {
        Ok(*offset)
    }
    fn readdir(&mut self) -> Result<Vec<DirEntry>> {
        Ok(self.entries.clone())
    }
}

// ---------------------------------------------------------------------------
// ProcFs — filesystem type registration
// ---------------------------------------------------------------------------

static PROCFS_ROOT: Once<Arc<ProcRootInode>> = Once::new();

/// Returns the singleton procfs root inode.
pub fn get_procfs_root() -> Arc<ProcRootInode> {
    PROCFS_ROOT.call_once(ProcRootInode::new).clone()
}

/// The `procfs` filesystem type.
///
/// Register this with the VFS using
/// [`register_filesystem`][crate::fs::vfs::register_filesystem] and then
/// mount it at `/proc`.
pub struct ProcFs;

impl FileSystem for ProcFs {
    fn name(&self) -> &'static str {
        "procfs"
    }

    fn mount(&self, _flags: u32, _data: &[u8]) -> Result<Arc<SuperBlock>> {
        let sb = Arc::new(SuperBlock {
            fs_type: String::from(self.name()),
            root_dentry: SpinLock::new(None),
        });
        let root_inode = get_procfs_root() as Arc<dyn InodeOps>;
        let root_dentry = Dentry::new("/", root_inode, None);
        *sb.root_dentry.lock() = Some(root_dentry);
        Ok(sb)
    }
}

// ---------------------------------------------------------------------------
// FdTable extension — list_fds
// ---------------------------------------------------------------------------
// The FdTable type needs a `list_fds()` method that procfs can call.
// We add a trait that the FdTable must implement; the actual method is
// implemented in fd_table.rs.  For now we call the public API directly.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::ramfs::RamFs;
    use crate::fs::vfs::{init_root_fs, mount, register_filesystem, resolve_path};
    use crate::proc::process::Process;
    use crate::vm::vma::VmaManager;
    use alloc::sync::Arc;
    use ostd::prelude::ktest;

    /// Helper: set up a minimal root + procfs mount, run `body`, then clean up.
    fn with_procfs<F: FnOnce()>(body: F) {
        let ramfs = Arc::new(RamFs);
        let _ = register_filesystem(ramfs);
        let _ = init_root_fs("ramfs", &[]);

        let procfs = Arc::new(ProcFs);
        let _ = register_filesystem(procfs);

        let root = crate::fs::vfs::ROOT_DENTRY
            .lock()
            .as_ref()
            .cloned()
            .unwrap();
        root.inode.mkdir("proc", 0o755).unwrap();
        mount("procfs", "/proc", 0, &[]).unwrap();

        body();

        crate::fs::vfs::unregister_filesystem("procfs").unwrap();
        crate::fs::vfs::unregister_filesystem("ramfs").unwrap();
        *crate::fs::vfs::ROOT_DENTRY.lock() = None;
        *crate::fs::vfs::CWD_DENTRY.lock() = None;
        crate::fs::vfs::DENTRY_CACHE.clear();
    }

    #[ktest]
    fn test_procfs_version() {
        with_procfs(|| {
            let dentry = resolve_path("/proc/version").unwrap();
            let meta = dentry.inode.metadata().unwrap();
            assert_eq!(meta.file_type, FileType::Regular);
            assert!(meta.size > 0);

            let mut file = dentry.inode.open(0).unwrap();
            let mut buf = alloc::vec![0u8; 128];
            let mut offset = 0;
            let n = file.read(&mut buf, &mut offset).unwrap();
            assert!(n > 0);
            let content = core::str::from_utf8(&buf[..n]).unwrap();
            assert!(
                content.contains("PetraOS"),
                "version file missing kernel name"
            );
        });
    }

    #[ktest]
    fn test_procfs_uptime() {
        with_procfs(|| {
            let dentry = resolve_path("/proc/uptime").unwrap();
            let mut file = dentry.inode.open(0).unwrap();
            let mut buf = alloc::vec![0u8; 64];
            let mut offset = 0;
            let n = file.read(&mut buf, &mut offset).unwrap();
            assert!(n > 0);
            let content = core::str::from_utf8(&buf[..n]).unwrap();
            // Should look like "123.45 0.00\n"
            assert!(content.contains('.'), "uptime missing decimal: {content}");
        });
    }

    #[ktest]
    fn test_procfs_process_status() {
        with_procfs(|| {
            // Create a test process so we have at least one PID in the table.
            let vm = Arc::new(VmaManager::new());
            let proc = Process::new(vm, "test-procfs");
            let pid = proc.pid.as_u32();

            let path = format!("/proc/{pid}/status");
            let dentry = resolve_path(&path).unwrap();
            assert_eq!(
                dentry.inode.metadata().unwrap().file_type,
                FileType::Regular
            );

            let mut file = dentry.inode.open(0).unwrap();
            let mut buf = alloc::vec![0u8; 512];
            let mut offset = 0;
            let n = file.read(&mut buf, &mut offset).unwrap();
            let content = core::str::from_utf8(&buf[..n]).unwrap();

            assert!(
                content.contains("Name:\ttest-procfs"),
                "status missing Name field"
            );
            assert!(content.contains("Pid:"), "status missing Pid field");
            assert!(content.contains("State:"), "status missing State field");
        });
    }

    #[ktest]
    fn test_procfs_process_stat() {
        with_procfs(|| {
            let vm = Arc::new(VmaManager::new());
            let proc = Process::new(vm, "stat-proc");
            let pid = proc.pid.as_u32();

            let path = format!("/proc/{pid}/stat");
            let dentry = resolve_path(&path).unwrap();
            let mut file = dentry.inode.open(0).unwrap();
            let mut buf = alloc::vec![0u8; 512];
            let mut offset = 0;
            let n = file.read(&mut buf, &mut offset).unwrap();
            let content = core::str::from_utf8(&buf[..n]).unwrap();

            // First field is the PID
            assert!(
                content.starts_with(&pid.to_string()),
                "stat first field should be PID, got: {content}"
            );
            // Second field is (name)
            assert!(content.contains("(stat-proc)"), "stat missing process name");
        });
    }

    #[ktest]
    fn test_procfs_root_readdir() {
        with_procfs(|| {
            let vm = Arc::new(VmaManager::new());
            let _proc = Process::new(vm, "readdir-proc");

            let dentry = resolve_path("/proc").unwrap();
            let mut dir = dentry.inode.open(0).unwrap();
            let entries = dir.readdir().unwrap();

            let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            assert!(names.contains(&"version"), "root missing 'version'");
            assert!(names.contains(&"uptime"), "root missing 'uptime'");
            assert!(names.contains(&"self"), "root missing 'self'");
            // At least one PID directory should appear
            assert!(
                entries
                    .iter()
                    .any(|e| e.file_type == FileType::Directory && e.name != "." && e.name != ".."),
                "root should contain at least one PID directory"
            );
        });
    }

    #[ktest]
    fn test_procfs_self_symlink() {
        with_procfs(|| {
            let vm = Arc::new(VmaManager::new());
            let _proc = Process::new(vm, "self-test");

            let proc_dentry = resolve_path("/proc").unwrap();
            let self_inode = proc_dentry.inode.lookup("self").unwrap();
            let meta = self_inode.metadata().unwrap();
            assert_eq!(meta.file_type, FileType::Symlink);

            let target = self_inode.read_link().unwrap();
            assert!(
                target.starts_with("/proc/"),
                "self symlink target: {target}"
            );
        });
    }
}
