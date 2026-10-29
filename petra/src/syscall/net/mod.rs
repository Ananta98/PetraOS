// SPDX-License-Identifier: GPL-2.0

//! UNIX Network System Calls implementation.
//!
//! Provides Linux-compatible network system calls split across modular subsystems:
//! - [`addr`]: Address parsing, serialization, and socket resolution
//! - [`socket`]: Socket creation (`socket(2)`, `socketpair(2)`)
//! - [`conn`]: Connection lifecycle (`bind(2)`, `connect(2)`, `listen(2)`, `accept(2)`, `accept4(2)`)
//! - [`send_recv`]: Basic transmission (`sendto(2)`, `recvfrom(2)`)
//! - [`msg`]: Advanced scatter-gather messaging (`sendmsg(2)`, `recvmsg(2)`, `sendmmsg(2)`, `recvmmsg(2)`)
//! - [`opts`]: Socket options (`setsockopt(2)`, `getsockopt(2)`)
//! - [`info`]: State query and termination (`getsockname(2)`, `getpeername(2)`, `shutdown(2)`)

pub mod addr;
pub mod conn;
pub mod info;
pub mod msg;
pub mod opts;
pub mod send_recv;
pub mod socket;

pub use addr::*;
pub use conn::*;
pub use info::*;
pub use msg::*;
pub use opts::*;
pub use send_recv::*;
pub use socket::*;
