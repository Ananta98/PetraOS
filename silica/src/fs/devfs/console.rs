use crate::api::errno::Result;
use crate::drivers::CharacterDevice;
use crate::fs::vfs::{FileOps, NodeOps, OpenFlags};
use alloc::{boxed::Box, sync::Arc};

/// /dev/console - Standard kernel and user console stream.
pub struct ConsoleFile;

impl FileOps for ConsoleFile {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        if let Some(cons) = crate::drivers::char::tty::get_console() {
            cons.read(buf)
        } else {
            Ok(0)
        }
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        if let Some(cons) = crate::drivers::char::tty::get_console() {
            cons.write(buf)
        } else {
            for &b in buf {
                ostd::early_print!("{}", b as char);
            }
            Ok(buf.len())
        }
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        if let Some(cons) = crate::drivers::char::tty::get_console() {
            cons.ioctl(cmd, arg)
        } else {
            crate::return_errno!(ENOTTY, "no console device available");
        }
    }

    fn poll(&self) -> crate::fs::poll::PollEvents {
        if let Some(cons) = crate::drivers::char::tty::get_console() {
            if let Ok(mask) = cons.poll() {
                return crate::fs::poll::PollEvents::from_bits_truncate(mask as u16);
            }
        }
        crate::fs::poll::PollEvents::IN | crate::fs::poll::PollEvents::OUT
    }
}

pub struct ConsoleNodeOps;

impl NodeOps for ConsoleNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(ConsoleFile))
    }
}

impl ConsoleFile {
    /// Helper to create a standard open File description for /dev/console with specific flags.
    pub fn new_file_with_flags(flags: OpenFlags) -> Arc<crate::fs::vfs::File> {
        let inode = Arc::new(crate::fs::vfs::INode::new(
            Box::new(ConsoleNodeOps),
            crate::fs::vfs::Mode::CHAR
                | crate::fs::vfs::Mode::RUSR
                | crate::fs::vfs::Mode::WUSR
                | crate::fs::vfs::Mode::RGRP
                | crate::fs::vfs::Mode::WGRP
                | crate::fs::vfs::Mode::ROTH
                | crate::fs::vfs::Mode::WOTH,
            2,
        ));
        Arc::new(crate::fs::vfs::File::new(
            Box::new(ConsoleFile),
            inode,
            flags,
            false,
        ))
    }

    /// Helper to create a standard open File description for /dev/console (R/W).
    pub fn new_file() -> Arc<crate::fs::vfs::File> {
        Self::new_file_with_flags(OpenFlags::READ | OpenFlags::WRITE)
    }
}
