//! Hardware IRQ primitives and top-half interrupt management.

use alloc::sync::Arc;
use ostd::irq::IrqLine;

/// Trait for device top-half interrupt handlers.
///
/// Implementors must be `Send + Sync + 'static` so the closure can be
/// invoked on any CPU and live for the lifetime of the kernel.
pub trait IrqHandler: Send + Sync + 'static {
    /// Called in the hardware interrupt top-half context.
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

impl IrqHandler for Arc<dyn IrqHandler> {
    fn handle(&self) {
        (**self).handle();
    }
}

/// Internal wrapper that unifies plain and mapped IRQ lines behind one type.
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

/// RAII owner of an allocated IRQ line with a registered top-half handler.
///
/// The line is released when this value is dropped.
#[must_use]
pub struct IrqRegistration {
    line: IrqLineContainer,
    _handler: Arc<dyn IrqHandler>,
}

impl IrqRegistration {
    /// Allocate any available IRQ line and associate `handler`.
    pub fn alloc_any(handler: impl IrqHandler) -> Result<Self, ostd::Error> {
        let mut line = IrqLine::alloc().map(IrqLineContainer::Plain)?;
        let handler = Arc::new(handler);
        let h = handler.clone();
        line.on_active(move |_| h.handle());
        Ok(Self {
            line,
            _handler: handler,
        })
    }

    /// Allocate a specific legacy IRQ number and register `handler`.
    pub fn alloc_specific(irq_num: u8, handler: impl IrqHandler) -> Result<Self, ostd::Error> {
        let mut line = IrqLine::alloc_specific(irq_num).map(IrqLineContainer::Plain)?;
        let handler = Arc::new(handler);
        let h = handler.clone();
        line.on_active(move |_| h.handle());
        Ok(Self {
            line,
            _handler: handler,
        })
    }

    /// Returns the IRQ vector number of the allocated line.
    pub fn num(&self) -> u8 {
        self.line.num()
    }

    /// Returns a reference to the underlying [`IrqLine`].
    pub fn irq_line(&self) -> &IrqLine {
        self.line.irq_line()
    }
}

/// Map an ISA legacy IRQ pin through the platform IRQ chip and register a
/// top-half handler (x86_64 only).
#[cfg(target_arch = "x86_64")]
pub fn map_isa_irq(isa_irq: u8, handler: impl IrqHandler) -> Result<IrqRegistration, ostd::Error> {
    use ostd::arch::irq::IRQ_CHIP;

    let irq_chip = IRQ_CHIP.get().ok_or(ostd::Error::NotEnoughResources)?;
    let raw_line = IrqLine::alloc()?;
    let mapped = irq_chip.map_isa_pin_to(raw_line, isa_irq)?;

    let handler = Arc::new(handler);
    let h = handler.clone();
    let mut line = IrqLineContainer::Mapped(mapped);
    line.on_active(move |_| h.handle());

    Ok(IrqRegistration {
        line,
        _handler: handler,
    })
}

/// RAII guard that disables local interrupts for a critical section.
pub type IrqGuard = ostd::irq::DisabledLocalIrqGuard;

/// Disable local IRQs and return an [`IrqGuard`].
pub fn disable_local() -> IrqGuard {
    ostd::irq::disable_local()
}
