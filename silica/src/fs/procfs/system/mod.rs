// SPDX-License-Identifier: GPL-2.0

//! Root-level synthetic system files for `/proc`.

pub mod cmdline;
pub mod cpuinfo;
pub mod meminfo;
pub mod mounts;
pub mod stat;
pub mod uptime;
pub mod version;

pub use cmdline::generate_cmdline;
pub use cpuinfo::generate_cpuinfo;
pub use meminfo::generate_meminfo;
pub use mounts::generate_mounts;
pub use stat::generate_stat;
pub use uptime::generate_uptime;
pub use version::generate_version;
