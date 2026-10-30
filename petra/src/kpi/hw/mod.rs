// SPDX-License-Identifier: GPL-2.0

//! Hardware access and interrupt abstractions: MMIO remapping and IRQ dispatch.

pub mod dma;
pub mod io;
pub mod irq;

pub use dma::{dma_addr_t, dma_alloc_coherent, dma_free_coherent, DmaBuffer};
pub use io::{ioremap, ioremap_shared, IoRegion};
pub use irq::{free_irq, request_irq, request_threaded_irq, IrqFlags, IrqHandlerFn, IrqReturn};
