//! IRQ interrupt management for device drivers.
//!
//! Provides safe abstractions over OSTD's interrupt line allocation,
//! callback registration, and IRQ chip mapping.
//!
//! # Architecture
//!
//! This layer sits between device drivers and the OSTD interrupt framework:
//!
//! - [`IrqRegistration`] — RAII IRQ line with a registered top-half handler.
//! - [`IrqHandler`] trait — simple `handle(&self)` interface for drivers.
//! - [`IrqGuard`] — RAII guard that disables local IRQs during critical sections.
//! - [`map_isa_irq`] — (x86-only) ISA IRQ → GSI mapping through the IRQ chip.
//!
//! # Example
//!
//! ```rust,ignore
//! use crate::drivers::irq::{IrqHandler, IrqRegistration};
//!
//! struct MyDriver;
//! impl IrqHandler for MyDriver {
//!     fn handle(&self) {
//!         // top-half handler logic
//!     }
//! }
//!
//! let reg = IrqRegistration::alloc_any(MyDriver).unwrap();
//! ```

pub mod msi;

use alloc::sync::Arc;

use ostd::irq::IrqLine;

// ---------------------------------------------------------------------------
// IrqHandler trait
// ---------------------------------------------------------------------------

/// The trait that device-level interrupt handlers should implement.
///
/// This is a simplified interface over the raw [`IrqLine::on_active`] API
/// that hides the [`TrapFrame`] parameter from device drivers.
///
/// # Requirements
///
/// Implementors must be `Send + Sync + 'static` so the handler can be
/// invoked on any CPU and can live for the lifetime of the kernel.
pub trait IrqHandler: Send + Sync + 'static {
    /// Invoked in the top half of interrupt handling.
    ///
    /// Interrupts are still disabled on the local CPU when this runs,
    /// so the handler should be as fast as possible. Defer heavy work
    /// to a bottom-half or tasklet mechanism.
    fn handle(&self);
}

impl<F> IrqHandler for F
where
    F: Fn() + Send + Sync + 'static,
{
    fn handle(&self) {
        (self)()
    }
}

// ---------------------------------------------------------------------------
// IrqLine container (avoids unsafe extraction from MappedIrqLine)
// ---------------------------------------------------------------------------

/// Internal container that holds either a plain [`IrqLine`] or an
/// architecture-specific mapped line.
enum IrqLineContainer {
    Plain(IrqLine),
    #[cfg(target_arch = "x86_64")]
    Mapped(ostd::arch::irq::MappedIrqLine),
}

impl IrqLineContainer {
    fn num(&self) -> u8 {
        match self {
            Self::Plain(line) => line.num(),
            #[cfg(target_arch = "x86_64")]
            Self::Mapped(line) => line.num(),
        }
    }

    fn on_active(
        &mut self,
        callback: impl Fn(&ostd::arch::trap::TrapFrame) + Sync + Send + 'static,
    ) {
        match self {
            Self::Plain(line) => line.on_active(callback),
            #[cfg(target_arch = "x86_64")]
            Self::Mapped(line) => line.on_active(callback),
        }
    }

    fn irq_line(&self) -> &IrqLine {
        match self {
            Self::Plain(line) => line,
            #[cfg(target_arch = "x86_64")]
            Self::Mapped(line) => line,
        }
    }
}

// ---------------------------------------------------------------------------
// IrqRegistration
// ---------------------------------------------------------------------------

/// An owned, active IRQ line with a registered top-half handler.
///
/// The IRQ line is allocated on construction and released (along with
/// automatic unregistration of all registered callbacks) on drop.
///
/// # Construction
///
/// | Method | Use case |
/// |--------|----------|
/// | [`IrqRegistration::alloc_any`] | Allocate any free IRQ (e.g., MSI, PCIe) |
/// | [`IrqRegistration::alloc_specific`] | Allocate a specific legacy IRQ number |
///
/// # Example
///
/// ```rust,ignore
/// let reg = IrqRegistration::alloc_any(|| {
///     info!("interrupt received!");
/// }).unwrap();
///
/// let reg = IrqRegistration::alloc_specific(1, || { ... }).unwrap();
/// ```
#[must_use]
pub struct IrqRegistration {
    line: IrqLineContainer,
    _handler: Arc<dyn IrqHandler>,
}

impl IrqRegistration {
    /// Allocate any available IRQ line and associate `handler`.
    ///
    /// The OSTD IRQ allocator picks the first free line (in the range
    /// defined by the architecture, e.g. 32–255 on x86).
    pub fn alloc_any(handler: impl IrqHandler) -> Result<Self, ostd::Error> {
        let mut line = IrqLine::alloc().map(IrqLineContainer::Plain)?;
        let handler = Arc::new(handler);
        let h = handler.clone();
        line.on_active(move |_| h.handle());
        Ok(Self { line, _handler: handler })
    }

    /// Allocate a specific legacy IRQ number (`irq_num`) and register
    /// `handler`.
    pub fn alloc_specific(irq_num: u8, handler: impl IrqHandler) -> Result<Self, ostd::Error> {
        let mut line = IrqLine::alloc_specific(irq_num).map(IrqLineContainer::Plain)?;
        let handler = Arc::new(handler);
        let h = handler.clone();
        line.on_active(move |_| h.handle());
        Ok(Self { line, _handler: handler })
    }

    /// Returns the IRQ number of the allocated line.
    pub fn num(&self) -> u8 {
        self.line.num()
    }

    /// Returns a reference to the underlying [`IrqLine`].
    ///
    /// This allows direct access to OSTD features (e.g., low-level
    /// remapping queries) when needed.
    pub fn irq_line(&self) -> &IrqLine {
        self.line.irq_line()
    }
}

// ---------------------------------------------------------------------------
// ISA IRQ mapping (x86 only)
// ---------------------------------------------------------------------------

/// Map an ISA legacy IRQ through the platform IRQ chip and return an
/// [`IrqRegistration`] with the handler attached.
///
/// This is **x86-specific**. It translates the legacy ISA interrupt number
/// (0–15) to a GSI (Global System Interrupt) through the I/O APIC and
/// enables the pin.
///
/// # Errors
///
/// Returns `ostd::Error::NotEnoughResources` if the IRQ chip is not yet
/// initialised or the GSI mapping fails.
#[cfg(target_arch = "x86_64")]
pub fn map_isa_irq(
    isa_irq: u8,
    handler: impl IrqHandler,
) -> Result<IrqRegistration, ostd::Error> {
    use ostd::arch::irq::IRQ_CHIP;

    let irq_chip = IRQ_CHIP.get().ok_or(ostd::Error::NotEnoughResources)?;
    // Allocate ANY free IrqLine from the OSTD pool (range 32–255 on x86),
    // then map its underlying interrupt to the ISA pin through the IRQ chip.
    // `alloc_specific(isa_irq)` would fail because ISA numbers (0–15) are
    // below the allocator min (32).
    let raw_line = IrqLine::alloc()?;
    let mapped = irq_chip.map_isa_pin_to(raw_line, isa_irq)?;

    let handler = Arc::new(handler);
    let h = handler.clone();
    let mut line = IrqLineContainer::Mapped(mapped);
    line.on_active(move |_| h.handle());

    Ok(IrqRegistration { line, _handler: handler })
}

// ---------------------------------------------------------------------------
// IrqGuard
// ---------------------------------------------------------------------------

/// RAII guard that disables local interrupts for the duration of a critical
/// section.
///
/// This wraps [`ostd::irq::DisabledLocalIrqGuard`] and is re-exported for
/// driver convenience.
pub type IrqGuard = ostd::irq::DisabledLocalIrqGuard;

/// Disable local IRQs and return an [`IrqGuard`].
///
/// Interrupts are re-enabled when the returned guard is dropped.
pub fn disable_local() -> IrqGuard {
    ostd::irq::disable_local()
}
