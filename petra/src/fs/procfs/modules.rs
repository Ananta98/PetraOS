// SPDX-License-Identifier: GPL-2.0

//! `/proc/modules` — loaded and builtin kernel modules.

use alloc::{format, string::String, sync::Arc, vec::Vec};

use crate::{
    fs::{
        procfs::template::{ProcFileOps, file_node},
        vfs::INode,
    },
    modules::list_modules,
};

pub struct ModulesFileOps;

impl ProcFileOps for ModulesFileOps {
    fn generate_content(&self) -> Vec<u8> {
        let mut out = String::new();

        // Builtin modules essential for systemd and standard userspace
        out.push_str("autofs4 0 0 - Live 0x0 (Builtin)\n");
        out.push_str("unix 0 0 - Live 0x0 (Builtin)\n");

        for m in list_modules() {
            let state_str = match m.state {
                crate::modules::ModuleState::Live => "Live",
                crate::modules::ModuleState::Unloading => "Unloading",
            };
            out.push_str(&format!(
                "{} {} 0 - {} {:#x}\n",
                m.name, m.load_size, state_str, m.load_base
            ));
        }

        out.into_bytes()
    }
}

impl ModulesFileOps {
    pub fn new_inode() -> Arc<INode> {
        file_node(Self, super::PROC_ROOT_INO_MODULES)
    }
}
