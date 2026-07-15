/// Driver abstraction — probe interface for hardware discovery.
///
/// Implement [`Driver`] for each hardware driver. The driver manager
/// calls [`Driver::probe`] during initialisation to detect and bind
/// hardware devices.

// ---------------------------------------------------------------------------
// Driver trait
// ---------------------------------------------------------------------------

/// Trait implemented by every hardware driver registered with the kernel.
pub trait Driver: Send + Sync {
    /// Human-readable name used as the key in the driver registry.
    fn name(&self) -> &str;

    /// Attempt to detect and initialise the hardware this driver controls.
    ///
    /// Returns `Ok(())` if the hardware was found and initialised
    /// successfully, or an error if detection fails.
    fn probe(&self) -> Result<(), ostd::Error>;
}
