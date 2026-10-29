// SPDX-License-Identifier: GPL-2.0

//! Linux-shaped interrupt management interface.
//!
//! Provides Linux-compatible `request_irq`, `free_irq`, interrupt return codes (`irqreturn_t`),
//! and bottom-half integration via `kpi::task::schedule_work`.
//!
//! # Safety
//! Interrupt lines and callbacks are managed through OSTD's architecture IRQ abstractions,
//! ensuring that handlers run cleanly under the appropriate interrupt level without leaking
//! unsound state into process context.

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use ostd::{
    arch::{
        irq::{IRQ_CHIP, MappedIrqLine},
        trap::TrapFrame,
    },
    irq::IrqLine,
};
use spin::{Mutex, RwLock};

use crate::uapi::errno::{Errno, Result};

/// Linux `irqreturn_t`: return value from interrupt handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqReturn {
    /// Interrupt was not handled by this device (`IRQ_NONE`).
    None,
    /// Interrupt was handled successfully (`IRQ_HANDLED`).
    Handled,
    /// Interrupt handler wants to wake a threaded bottom half (`IRQ_WAKE_THREAD`).
    WakeThread,
}

/// Linux `IRQF_*` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IrqFlags(u32);

impl IrqFlags {
    /// No flags.
    pub const NONE: Self = Self(0);
    /// Allow sharing IRQ with other handlers (`IRQF_SHARED`).
    pub const SHARED: Self = Self(1 << 7);
    /// Run handler with local interrupts disabled.
    pub const NO_SUSPEND: Self = Self(1 << 10);

    /// Combines two sets of flags.
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Checks if shared flag is present.
    pub fn is_shared(self) -> bool {
        self.0 & Self::SHARED.0 != 0
    }
}

/// A fast interrupt service routine (top-half), mirroring Linux `irq_handler_t`.
pub type IrqHandlerFn = Box<dyn Fn() -> IrqReturn + Send + Sync>;

/// Bottom-half threaded or deferred handler.
pub type IrqThreadFn = Arc<dyn Fn() + Send + Sync>;

struct IrqAction {
    name: String,
    handler: IrqHandlerFn,
    thread_fn: Option<IrqThreadFn>,
    flags: IrqFlags,
}

struct IrqLineState {
    irq_number: usize,
    mapped_line: Option<MappedIrqLine>,
    actions: Vec<IrqAction>,
}

/// Internal IRQ registry managing registered interrupt lines.
static IRQ_REGISTRY: RwLock<Vec<Arc<Mutex<IrqLineState>>>> = RwLock::new(Vec::new());

fn dispatch_irq(irq_num: usize) {
    let state_arc = {
        let registry = IRQ_REGISTRY.read();
        registry
            .iter()
            .find(|entry| entry.lock().irq_number == irq_num)
            .cloned()
    };

    let mut wake_threads = Vec::new();
    if let Some(state_arc) = state_arc {
        let guard = state_arc.lock();
        for action in guard.actions.iter() {
            let res = (action.handler)();
            if res == IrqReturn::WakeThread {
                if let Some(ref thread_fn) = action.thread_fn {
                    wake_threads.push(Arc::clone(thread_fn));
                }
            }
        }
    }

    for task_fn in wake_threads {
        crate::kpi::task::schedule_work_fn(move || {
            task_fn();
        });
    }
}

/// Linux `request_irq`: allocates and binds an interrupt line to a handler.
pub fn request_irq<F>(
    irq_num: usize,
    handler: F,
    flags: IrqFlags,
    dev_name: impl Into<String>,
) -> Result<()>
where
    F: Fn() -> IrqReturn + Send + Sync + 'static,
{
    request_threaded_irq(irq_num, handler, None::<fn()>, flags, dev_name)
}

/// Linux `request_threaded_irq`: binds top-half and bottom-half handlers.
pub fn request_threaded_irq<H, T>(
    irq_num: usize,
    handler: H,
    thread_fn: Option<T>,
    flags: IrqFlags,
    dev_name: impl Into<String>,
) -> Result<()>
where
    H: Fn() -> IrqReturn + Send + Sync + 'static,
    T: Fn() + Send + Sync + 'static,
{
    let name_str = dev_name.into();
    let thread_arc: Option<IrqThreadFn> = thread_fn.map(|f| Arc::new(f) as IrqThreadFn);

    let action = IrqAction {
        name: name_str,
        handler: Box::new(handler),
        thread_fn: thread_arc,
        flags,
    };

    let mut registry = IRQ_REGISTRY.write();
    if let Some(existing) = registry.iter().find(|e| e.lock().irq_number == irq_num) {
        let mut guard = existing.lock();
        if !flags.is_shared() || guard.actions.iter().any(|a| !a.flags.is_shared()) {
            crate::return_errno!(EBUSY, "irq line is not shared");
        }
        guard.actions.push(action);
        return Ok(());
    }

    // Allocate new IRQ line mapping via OSTD IRQ chip if available
    let mut mapped_line = None;
    if let Some(chip) = IRQ_CHIP.get() {
        if let Ok(line) = IrqLine::alloc() {
            if let Ok(mut mapped) = chip.map_isa_pin_to(line, irq_num as u8) {
                mapped.on_active(move |_frame: &TrapFrame| {
                    dispatch_irq(irq_num);
                });
                mapped_line = Some(mapped);
            }
        }
    }

    let state = IrqLineState {
        irq_number: irq_num,
        mapped_line,
        actions: alloc::vec![action],
    };

    registry.push(Arc::new(Mutex::new(state)));
    Ok(())
}

/// Linux `free_irq`: releases a registered interrupt handler.
pub fn free_irq(irq_num: usize, dev_name: &str) -> Result<()> {
    let mut registry = IRQ_REGISTRY.write();
    let index = registry
        .iter()
        .position(|e| e.lock().irq_number == irq_num)
        .ok_or(Errno::ENODEV)?;

    let mut guard = registry[index].lock();
    guard.actions.retain(|action| action.name != dev_name);

    if guard.actions.is_empty() {
        drop(guard);
        registry.remove(index);
    }

    Ok(())
}
