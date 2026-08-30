//! Network Subsystem for PetraOS
//!
//! Integrates smoltcp TCP/IP stack, generic network device drivers, UNIX domain sockets,
//! and network socket lifecycle.

pub mod device;
pub mod socket;
pub mod stack;
pub mod types;

pub use device::NetDeviceAdapter;
pub use socket::Socket;
pub use stack::{NET_STACK, NetworkStack, current_time, poll};
pub use types::*;

/// Initialize the kernel network stack and attach available network devices.
pub fn init() -> Result<(), &'static str> {
    log::info!("Initializing PetraOS Network Subsystem...");
    if let Some(stack) = NetworkStack::new() {
        *NET_STACK.lock() = Some(stack);
        log::info!("[Net] Network stack initialized with IPv4 (10.0.2.15/24) and IPv6.");
    } else {
        log::warn!("[Net] No active network device discovered for IP stack initialization.");
    }
    Ok(())
}

crate::device_initcall!(crate::net::init);
