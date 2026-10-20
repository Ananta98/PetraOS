// SPDX-License-Identifier: GPL-2.0

//! Process and thread lifecycle operations.

pub mod clone;
pub mod exec;
pub mod exit;
pub mod wait;

pub use self::{
    clone::{
        CLONE_CHILD_CLEARTID, CLONE_CHILD_SETTID, CLONE_FILES, CLONE_FS, CLONE_PARENT_SETTID,
        CLONE_SETTLS, CLONE_SIGHAND, CLONE_THREAD, CLONE_VFORK, CLONE_VM, CSIGNAL, clone_proc,
        clone_thread, create_user_thread, fork1,
    },
    exec::{
        AT_EMPTY_PATH, AT_FDCWD, AT_SYMLINK_NOFOLLOW, DEFAULT_INTERP_LOAD_BIAS,
        DEFAULT_PIE_LOAD_BIAS, ElfLoadInfo, USER_STACK_BASE, USER_STACK_SIZE, USER_STACK_TOP,
        execve, execve_at, kern_execve, load_and_setup, load_and_setup_full, load_elf,
        load_elf_with_bias, read_binary_data, read_executable_file, setup_user_stack,
        setup_user_stack_full,
    },
    exit::{exit_thread, exit1, proc_reap},
    wait::{IdType, WaitOptions, WaitResult, kernel_wait, notify_child_events},
};
