// SPDX-License-Identifier: GPL-2.0

//! Network Subsystem for PetraOS using smoltcp.

pub mod device;
pub mod interface;
pub mod netlink;
pub mod socket;
pub mod unix;
pub mod wireguard;

use crate::drivers::net::NetDevice;
use crate::uapi::errno::Result;
use alloc::sync::Arc;

pub use interface::{NET_MANAGER, poll};
pub use wireguard::{WireGuardDevice, WireGuardPeerConfig, register_wireguard_interface};

/// Attaches a network device to the global network stack interface.
pub fn attach_device(dev: Arc<dyn NetDevice>) {
    NET_MANAGER.lock().attach_device(dev);
}

/// Detaches a network device by name from the global network stack interface.
pub fn detach_device(name: &str) {
    NET_MANAGER.lock().detach_device(name);
}

/// Background worker thread that continuously polls network devices and dispatches events.
fn net_poller_entry() {
    loop {
        let changed = crate::net::poll();
        if changed {
            ostd::task::Task::yield_now();
        } else {
            crate::kpi::time::arm(core::time::Duration::from_millis(1), None).wait();
        }
    }
}

/// Spawns the background network polling daemon (`net_poller`).
pub fn init_net_daemon() {
    let _ = crate::proc::kthread::kproc_create("net_poller", net_poller_entry, -5)
        .expect("failed to spawn net_poller daemon");
    ostd::info!("Net: Background network polling daemon started");
}

/// Initializes the network subsystem.
pub fn init() -> Result<()> {
    ostd::info!("Net: Network subsystem initialized");
    Ok(())
}

crate::module!(
    "PetraOS Network Subsystem",
    "Ananta98",
    crate::modules::InitcallLevel::Core,
    init
);
