/// Driver abstraction — Bus-Driven Matching interface and lifecycle.
///
/// Implement [`Driver`] for each hardware driver. Drivers register on a specific
/// bus (e.g., "pci", "platform", "virtual") and participate in Bus-Driven Matching.

use crate::device::device::Device;
use alloc::sync::Arc;

/// Trait implemented by hardware drivers registered with the kernel.
pub trait Driver: Send + Sync {
    /// Human-readable name used as the key in the driver registry.
    fn name(&self) -> &str;

    /// Name of the target bus this driver attaches to (e.g. "pci", "platform", "virtual").
    fn bus_name(&self) -> &str {
        "platform"
    }

    /// Short description of the driver for module metadata.
    fn description(&self) -> &str {
        "Kernel device driver"
    }

    /// Bus-Driven Matching predicate evaluating whether this driver can manage `device`.
    fn match_device(&self, device: &Arc<dyn Device>) -> bool {
        device.name().contains(self.name())
    }

    /// Probes and binds a matched device instance.
    fn probe_device(&self, device: Arc<dyn Device>) -> Result<(), ostd::Error> {
        let _ = device;
        Ok(())
    }

    /// Attempt to detect and initialise hardware directly.
    fn probe(&self) -> Result<(), ostd::Error> {
        Ok(())
    }
}
