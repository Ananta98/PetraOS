// SPDX-License-Identifier: GPL-2.0

//! Input device nodes (`/dev/input/eventN`, `/dev/input/uinput`).
//!
//! The directory resolves names against the input core registry, so an
//! `eventN` node exists as soon as a driver registers its [`InputDevice`] and
//! disappears with it — no fixed enumeration, the same idea as `/dev/dri`.
//!
//! `eventN` files are evdev clients: each `open` gets its own event queue, so
//! several readers can consume the same device independently (X and a console
//! keyboard handler, say).

use alloc::{boxed::Box, format, sync::Arc, vec::Vec};

use crate::{
    drivers::input::{
        self, InputDevice,
        evdev::{EvdevClient, open as open_evdev},
        uinput::UinputFile,
    },
    fs::{
        poll::PollEvents,
        vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
    },
    uapi::errno::{Errno, Result},
};

/// Base inode number of `eventN` nodes.
const EVDEV_INO_BASE: usize = 200;
/// Inode number of `uinput`.
const UINPUT_INO: usize = 14;
/// Inode number of `mice`.
const MICE_INO: usize = 15;

/// `/dev/input` directory node.
pub struct InputDirNodeOps;

impl NodeOps for InputDirNodeOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        if name == "uinput" {
            return Ok(Arc::new(INode::new(
                Box::new(UinputNodeOps),
                Mode::CHAR | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
                UINPUT_INO,
            )));
        }

        if name == "mice" || name.starts_with("mouse") {
            return Ok(Arc::new(INode::new(
                Box::new(MiceNodeOps),
                Mode::CHAR | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
                MICE_INO,
            )));
        }

        let index = name
            .strip_prefix("event")
            .and_then(|idx| idx.parse::<usize>().ok())
            .ok_or(Errno::ENOENT)?;
        let device = input::get_device(index).ok_or(Errno::ENOENT)?;

        Ok(Arc::new(INode::new(
            Box::new(EvdevNodeOps { device }),
            Mode::CHAR | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
            EVDEV_INO_BASE + index,
        )))
    }

    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(InputDirFileOps))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::DIR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::XOTH,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// Lists `eventN`, `mice`, and `uinput` under `/dev/input`.
pub struct InputDirFileOps;

impl FileOps for InputDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from a directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write raw bytes to a directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(b"uinput\n");
        out.extend_from_slice(b"mice\n");
        out.extend_from_slice(b"mouse0\n");
        for device in input::devices() {
            out.extend_from_slice(format!("{}\n", device.node_name()).as_bytes());
        }
        Ok(out)
    }
}

/// An `eventN` char device inode: every open is a new evdev client.
pub struct EvdevNodeOps {
    device: Arc<InputDevice>,
}

impl NodeOps for EvdevNodeOps {
    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(EvdevFileOps {
            client: open_evdev(Arc::clone(&self.device)),
            nonblock: flags.contains(OpenFlags::NONBLOCK),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::CHAR | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// Per-open evdev state: one client queue and the reader's blocking mode.
pub struct EvdevFileOps {
    client: Arc<EvdevClient>,
    nonblock: bool,
}

impl FileOps for EvdevFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.client.read(buf, self.nonblock)
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        self.client.write(buf)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        self.client.ioctl(cmd, arg)
    }

    fn poll(&self) -> PollEvents {
        if self.client.available() > 0 {
            PollEvents::IN | PollEvents::RDNORM
        } else {
            PollEvents::empty()
        }
    }

    fn close(&self) -> Result<()> {
        Ok(())
    }
}

/// `/dev/input/mice` node: multiplexes mouse inputs as standard PS/2 packets.
pub struct MiceNodeOps;

impl NodeOps for MiceNodeOps {
    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(MiceFileOps {
            nonblock: flags.contains(OpenFlags::NONBLOCK),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::CHAR | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// Per-open state for `/dev/input/mice`.
pub struct MiceFileOps {
    nonblock: bool,
}

impl FileOps for MiceFileOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        crate::drivers::input::mousedev::read_mice(buf, self.nonblock)
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "mice device is read-only");
    }

    fn poll(&self) -> PollEvents {
        crate::drivers::input::mousedev::poll_mice()
    }
}


/// `/dev/input/uinput`: creates virtual input devices from userspace.
pub struct UinputNodeOps;

impl NodeOps for UinputNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(UinputFileOps {
            file: UinputFile::new(),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::CHAR | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// VFS adapter for [`UinputFile`].
pub struct UinputFileOps {
    file: UinputFile,
}

impl FileOps for UinputFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EINVAL, "uinput devices are write-only");
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        self.file.write(buf)
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        self.file.ioctl(cmd, arg)
    }

    fn poll(&self) -> PollEvents {
        PollEvents::OUT | PollEvents::WRNORM
    }
}
