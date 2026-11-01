// SPDX-License-Identifier: GPL-2.0

//! `/proc/sys/fs` parameters.

use alloc::{format, string::String, sync::Arc, vec::Vec};
use spin::RwLock;

use crate::{
    fs::{
        procfs::template::{ProcDirOps, ProcFileOps, rw_file_node},
        vfs::INode,
    },
    uapi::errno::Result,
};

const PROC_SYS_FS_INO_FILE_MAX: usize = 2020;
const PROC_SYS_FS_INO_NR_OPEN: usize = 2021;

static FILE_MAX: RwLock<u64> = RwLock::new(9223372036854775807);
static NR_OPEN: RwLock<u32> = RwLock::new(1073741816);

struct FileMaxFileOps;

impl ProcFileOps for FileMaxFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let val = *FILE_MAX.read();
        format!("{}\n", val).into_bytes()
    }

    fn write_content(&self, data: &[u8]) -> Result<()> {
        if let Ok(text) = core::str::from_utf8(data) {
            if let Ok(num) = text.trim().parse::<u64>() {
                *FILE_MAX.write() = num;
            }
        }
        Ok(())
    }
}

struct NrOpenFileOps;

impl ProcFileOps for NrOpenFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let val = *NR_OPEN.read();
        format!("{}\n", val).into_bytes()
    }

    fn write_content(&self, data: &[u8]) -> Result<()> {
        if let Ok(text) = core::str::from_utf8(data) {
            if let Ok(num) = text.trim().parse::<u32>() {
                *NR_OPEN.write() = num;
            }
        }
        Ok(())
    }
}

pub struct ProcSysFsDirOps;

impl ProcDirOps for ProcSysFsDirOps {
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>> {
        match name {
            "file-max" => Ok(rw_file_node(FileMaxFileOps, PROC_SYS_FS_INO_FILE_MAX)),
            "nr_open" => Ok(rw_file_node(NrOpenFileOps, PROC_SYS_FS_INO_NR_OPEN)),
            _ => crate::return_errno!(ENOENT, "unknown entry in /proc/sys/fs"),
        }
    }

    fn list_entries(&self) -> Vec<String> {
        alloc::vec![String::from("file-max"), String::from("nr_open")]
    }
}

