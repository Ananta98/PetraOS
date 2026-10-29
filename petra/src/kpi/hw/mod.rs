// SPDX-License-Identifier: GPL-2.0

//! Hardware access and interrupt abstractions: MMIO remapping and IRQ dispatch.

pub mod io;
pub mod irq;

pub use io::{ioremap, ioremap_shared, IoRegion};
pub use irq::{free_irq, request_irq, request_threaded_irq, IrqFlags, IrqHandlerFn, IrqReturn};
