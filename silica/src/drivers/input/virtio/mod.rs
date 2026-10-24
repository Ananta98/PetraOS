// SPDX-License-Identifier: GPL-2.0

//! VirtIO Input Device Driver (`virtio 1.1` §5.8).
//!
//! Provides support for QEMU / KVM VirtIO input devices (keyboards, mice,
//! tablets, multi-touch devices).

pub mod device;
pub mod protocol;

pub use device::VirtioInput;
pub use protocol::*;

use alloc::{sync::Arc, vec::Vec};
use ostd::sync::{LocalIrqDisabled, SpinLock};

use crate::{
    drivers::bus::virtio::{
        VIRTIO_F_VERSION_1, VirtioDevice, VirtioTransport, register_virtio_device,
    },
    modules::InitcallLevel,
    uapi::errno::Result,
};

/// Global registry of active `VirtioInput` devices.
static VIRTIO_INPUT_DEVICES: SpinLock<Vec<Arc<VirtioInput>>, LocalIrqDisabled> =
    SpinLock::new(Vec::new());

pub(crate) fn register_active_device(dev: Arc<VirtioInput>) {
    VIRTIO_INPUT_DEVICES.lock().push(dev);
}

/// Polls all registered `VirtioInput` devices for pending events in `eventq`.
pub fn poll_all() {
    let devices = VIRTIO_INPUT_DEVICES.lock().clone();
    for dev in devices {
        dev.poll_events();
    }
}

/// VirtIO bus registration driver for `virtio-input`.
pub struct VirtioInputDriver;

impl VirtioDevice for VirtioInputDriver {
    fn device_id(&self) -> u16 {
        PCI_DEVICE_ID_VIRTIO_INPUT
    }

    fn name(&self) -> &'static str {
        "virtio-input"
    }

    fn queue_count(&self) -> usize {
        QUEUE_COUNT
    }

    fn accept_features(&self, device_features: u64) -> u64 {
        device_features & VIRTIO_F_VERSION_1
    }

    fn on_init(&self, transport: &Arc<VirtioTransport>) -> Result<()> {
        let _dev = VirtioInput::new(transport)?;
        Ok(())
    }
}

/// Background worker thread that periodically polls VirtIO input queues (100 Hz).
fn virtio_input_poller_entry() {
    loop {
        poll_all();
        crate::kpi::time::arm(core::time::Duration::from_millis(10), None).wait();
    }
}

/// Initializes the `virtio-input` driver, registering with the VirtIO bus
/// and starting the background event polling worker.
pub fn init() -> Result<()> {
    register_virtio_device(Arc::new(VirtioInputDriver))?;
    let _ = crate::proc::kthread::kproc_create("virtio_input", virtio_input_poller_entry, 0);
    Ok(())
}

crate::module!(
    "Virtio Input Driver",
    "Ananta",
    InitcallLevel::Device,
    init
);
