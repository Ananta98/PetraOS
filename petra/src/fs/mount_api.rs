// SPDX-License-Identifier: GPL-2.0

//! `fsopen(2)`-family filesystem context objects.
//!
//! Mirrors Linux's `fs_context` lifecycle:
//!
//! - `fsopen(2)` hands out a context in the *unconfigured* state;
//! `fsconfig(2)` sets parameters; `FSCONFIG_CMD_CREATE` finishes the
//! configuration; `fsmount(2)` turns it into a *detached* mount;
//! `move_mount(2)` binds it onto a path, where the legacy `mount(2)` work
//! runs.
//! - `open_tree(2)` (without `OPEN_TREE_CLONE`) produces a *bind-source*
//! context over an existing path; `move_mount(2)` binds it as `MS_BIND`.
//! - `fspick(2)` produces a *picked* context to reconfigure an existing
//! mount (`FSCONFIG_CMD_RECONFIGURE`).


use alloc::string::String;
use alloc::sync::Arc;
use alloc::boxed::Box;
use ostd::sync::SpinLock;

use crate::fs::poll::PollEvents;
use crate::fs::vfs::{
    File, FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags, SeekAnchor,
};
use crate::uapi::errno::{Errno, Result};

/// `FSOPEN_CLOEXEC` / `FSMOUNT_CLOEXEC`.
pub const FSMOUNT_CLOEXEC: u32 = 1;
/// `OPEN_TREE_CLONE` — we do not provide mount cloning; `EINVAL`.
pub const OPEN_TREE_CLONE: u32 = 1;
/// `OPEN_TREE_CLOEXEC = O_CLOEXEC`.
pub const OPEN_TREE_CLOEXEC: u32 = 0o2000000;
/// `MOUNT_ATTR_RDONLY`.
pub const MOUNT_ATTR_RDONLY: u32 = 1;
/// `FSPICK_CLOEXEC`.
pub const FSPICK_CLOEXEC: u32 = 1;
/// The known `fsconfig(2)` commands, in uapi numbering.
pub const FSCONFIG_SET_FLAG: u32 = 0;
pub const FSCONFIG_SET_STRING: u32 = 1;
pub const FSCONFIG_SET_BINARY: u32 = 2;
pub const FSCONFIG_SET_PATH: u32 = 3;
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
pub const FSCONFIG_SET_FD: u32 = 5;
pub const FSCONFIG_CMD_CREATE: u32 = 6;
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;

/// Parameters gathered while a context is configured.
#[derive(Debug, Default)]
pub struct MountParams {
    pub fstype: String,
    pub source: String,
    pub ro: bool,
    pub data: String,
}

/// The lifecycle state a context is in.
pub enum FsCtxKind {
    FsCreate {
        params: MountParams,
        created: bool,
    },
   /// A bind source from open_tree(2); `move_mount(2)` mounts it MS_BIND.
    Bind {
        source_path: String,
    },
    /// An existing mount opened for reconfiguration (fspick(2)).
    Pick {
        path: String,
    },
    /// Configured by fsmount(2), waiting for move_mount(2) to bind it.
    Detached {
        params: MountParams,
    },
    /// Bound already; one-shot.
    Spent,
}

/// One filesystem context, held by its `fsopen(2)` fd.
pub struct FsContext {
    pub kind: SpinLock<FsCtxKind>,
}

impl FsContext {
    pub fn new(fstype: &str) -> Arc<Self> {
        Arc::new(Self {
            kind: SpinLock::new(FsCtxKind::FsCreate {
                params: MountParams {
                    fstype: String::from(fstype),
                    ..MountParams::default()
                },
                created: false,
            }),
        })
    }

    /// Registers a bind-source over `path` (open_tree(2) without cloning).
    pub fn new_bind(path: &str) -> Arc<Self> {
        Arc::new(Self {
            kind: SpinLock::new(FsCtxKind::Bind {
                source_path: String::from(path),
            }),
        })
    }

    /// Registers an fspick(2) reconfiguration context over `path`.
    pub fn new_pick(path: &str) -> Arc<Self> {
        Arc::new(Self {
            kind: SpinLock::new(FsCtxKind::Pick {
                path: String::from(path),
            }),
        })
    }

    /// Applies one `fsconfig(2)` command to the context.
    pub fn fsconfig(&self, cmd: u32, key: &str, value: &str) -> Result<()> {
        match cmd {
            FSCONFIG_SET_FLAG => match key {
                "ro" => self.set_flag_ro(true),
                "rw" => self.set_flag_ro(false),
                "silent" | "atime" => Ok(()),
                _ => crate::return_errno!(EINVAL, "unknown fsconfig flag"),
            },
            FSCONFIG_SET_STRING => match key {
                "source" => self.set_source(value),
                _ => crate::return_errno!(EINVAL, "unknown fsconfig parameter"),
            },
            FSCONFIG_CMD_CREATE => {
                let mut kind = self.kind.lock();
                match &mut *kind {
                    FsCtxKind::FsCreate { params, created } => {
                        if *created {
                            crate::return_errno!(EBUSY, "fs context already created");
                        }
                        if params.fstype.is_empty() {
                            crate::return_errno!(EINVAL, "filesystem type not configured");
                        }
                        *created = true;
                        Ok(())
                    }
                    _ => Err(Errno::EINVAL),
                }
            }
            FSCONFIG_CMD_RECONFIGURE => match &*self.kind.lock() {
                FsCtxKind::Pick { .. } => Ok(()),
                _ => Err(Errno::EINVAL),
            },
            // Binary/passthrough parameter classes have no in-kernel use here.
            FSCONFIG_SET_BINARY | FSCONFIG_SET_PATH | FSCONFIG_SET_PATH_EMPTY
            | FSCONFIG_SET_FD => Err(Errno::ENOTSUP),
            _ => Err(Errno::EINVAL),
        }
    }

    fn set_flag_ro(&self, ro: bool) -> Result<()> {
        let mut kind = self.kind.lock();
        match &mut *kind {
            FsCtxKind::FsCreate { params, .. } | FsCtxKind::Detached { params, .. } => {
                params.ro = ro;
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    fn set_source(&self, source: &str) -> Result<()> {
        let mut kind = self.kind.lock();
        match &mut *kind {
            FsCtxKind::FsCreate { params, .. } | FsCtxKind::Detached { params, .. } => {
                params.source = String::from(source);
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    /// Converts a created fscreate context into a detached mount context.
    pub fn fsmount(&self, attr_flags: u32) -> Result<()> {
        if attr_flags & !(MOUNT_ATTR_RDONLY | 0x2 | 0x4 | 0x8) != 0 {
            crate::return_errno!(EINVAL, "unknown mount attribute");
        }
        let mut kind = self.kind.lock();
        let FsCtxKind::FsCreate { params, created } = &mut *kind else {
            crate::return_errno!(EINVAL, "fsmount not valid for this context");
        };
        if !*created {
            crate::return_errno!(EBUSY, "fs context not created yet");
        }
        *kind = FsCtxKind::Detached {
            params: MountParams {
                fstype: String::clone(&params.fstype),
                source: String::clone(&params.source),
                ro: params.ro || (attr_flags & MOUNT_ATTR_RDONLY) != 0,
                data: String::clone(&params.data),
            },
        };
        Ok(())
    }

    /// Consumes a detached/bind context, returning the mount to perform.
    ///
    /// The context moves to [`FsCtxKind::Spent`]; binding twice is `EINVAL`.
    pub fn take_for_bind(&self) -> Result<(String, String, String, usize, bool)> {
        let mut kind = self.kind.lock();
        match core::mem::replace(&mut *kind, FsCtxKind::Spent) {
            FsCtxKind::Detached { params } => {
                let ro = params.ro;
                Ok((params.source, params.fstype, params.data, 0, ro))
            }
            FsCtxKind::Bind { source_path } => Ok((
                source_path,
                String::new(),
                String::new(),
                crate::syscall::fs::MS_BIND,
                false,
            )),
            _ => Err(Errno::EINVAL),
        }
    }

    /// Mount path of a picked context, for remount application.
    pub fn picked_path(&self) -> Result<String> {
        match &*self.kind.lock() {
            FsCtxKind::Pick { path } => Ok(String::clone(path)),
            _ => Err(Errno::EINVAL),
        }
    }
}

/// The file type that owns an [`FsContext`].
pub const FS_CTX_INODE_NUMBER: usize = 905;

struct FsCtxNodeOps;

impl NodeOps for FsCtxNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        crate::return_errno!(EOPNOTSUPP, "cannot directly open an fs context")
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::FIFO | Mode::RUSR | Mode::WUSR,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

struct FsCtxFileOps {
    ctx: Arc<FsContext>,
}

impl FileOps for FsCtxFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        Err(Errno::EINVAL)
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        Err(Errno::EINVAL)
    }

    fn seek(&self, _curr_offset: u64, _anchor: SeekAnchor) -> Result<u64> {
        Err(Errno::ESPIPE)
    }

    fn poll(&self) -> PollEvents {
        PollEvents::empty()
    }

    fn as_fsctx(&self) -> Option<Arc<FsContext>> {
        Some(self.ctx.clone())
    }
}

/// Builds the anonymous [`File`] an `fsopen(2)` / `fspick(2)` / `open_tree(2)`
/// call hands out.
pub fn create_fsctx_file(ctx: Arc<FsContext>, cloexec: bool) -> Arc<File> {
    let inode = Arc::new(INode::new(
        Box::new(FsCtxNodeOps),
        Mode::FIFO | Mode::RUSR | Mode::WUSR,
        FS_CTX_INODE_NUMBER,
    ));

    let mut open_flags = OpenFlags::READ | OpenFlags::WRITE;
    if cloexec {
        open_flags |= OpenFlags::CLOEXEC;
    }

    Arc::new(File::new(
        Box::new(FsCtxFileOps { ctx }),
        inode,
        open_flags,
        false,
    ))
}