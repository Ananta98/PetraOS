# Asterinas OSTD Reference Guide

Asterinas OSTD (Operating System Standard Library) is a framework that provides safe Rust abstractions over low-level hardware mechanisms, enabling operating system kernels (such as PetraOS `silica`) to be built with high memory safety, modularity, and code reuse.

---

## 1. Documentation & Resource Locations

- **Online API Docs**: [https://asterinas.github.io/api-docs](https://asterinas.github.io/api-docs)
- **Crate Documentation**: [https://docs.rs/ostd/0.18.1](https://docs.rs/ostd/0.18.1)
- **Asterinas Book**: [https://asterinas.github.io/book](https://asterinas.github.io/book)
- **Local Documentation Generation**:
  ```bash
  export PATH="$HOME/.cargo/bin:$PATH"
  cargo osdk doc
  ```
  HTML output is located in `target/osdk/doc/ostd/index.html`.
- **Local Crate Source**:
  Inspecting installed OSTD source code on this system:
  `~/.cargo/registry/src/index.crates.io-*/ostd-0.18.1/src/`

---

## 2. Core Subsystems & Safe APIs

### `ostd::prelude`
Commonly imported items for kernel development:
```rust
use ostd::prelude::*;
```

### `ostd::mm` (Memory Management)
Encapsulates physical memory frames, virtual address spaces, and page tables:
- `PAddr`: Physical memory address type.
- `VAddr`: Virtual memory address type.
- `Frame`: Safe RAII wrapper for allocated physical frames.
- `PageTable`: Architecture-independent page table management.
- `VMRange`: Virtual memory range abstraction.
- Custom memory allocators implementing the Rust `Allocator` API.

### `ostd::sync` (Synchronization Primitives)
Kernel-safe synchronization primitives designed for single-core and SMP kernels:
- `SpinLock<T>`: Spinlock with interrupt-disabling options.
- `Mutex<T>`: Blocking mutex for kernel threads.
- `RwLock<T>`: Reader-writer lock for concurrent data access.
- `WaitQueue`: Primitive for sleeping and waking tasks.

### `ostd::task` (Task & Threading)
Primitives for kernel execution contexts:
- Thread creation, state transitions, and context switching.
- Integration points for custom schedulers (e.g., `silica/src/sched`).

### `ostd::irq` (Interrupt Handling)
- Safe registration and dispatching of hardware interrupts.
- Functions to disable and restore CPU interrupt flags: `ostd::irq::disable_local()`.

### `ostd::io` & `ostd::bus` (I/O & Hardware Access)
- `IoPort`: Safe x86 I/O port reading (`inb`, `inw`, `inl`) and writing (`outb`, `outw`, `outl`).
- `Mmio<T>`: Safe memory-mapped I/O access with volatile semantics.
- `ostd::bus`: Bus abstractions including PCI device enumeration and configuration.

### `ostd::cpu` & `ostd::smp`
- CPU identification, feature discovery (CPUID on x86_64), and local APIC management.
- Per-CPU data structures and inter-processor interrupts (IPI).

### `ostd::timer`
- Access to high-resolution hardware timers (TSC, APIC timer).
- Timer calibration, elapsed time queries, and timeout events.

### `ostd::user` (Userspace Boundary)
- Safe access to user-space memory pointers without crashing the kernel.
- Boundary checks and safe byte copying between user and kernel spaces.

### `ostd::log` & `ostd::console`
- Standard kernel logging macros:
  ```rust
  use ostd::log::{debug, error, info, trace, warn};
  info!("Subsystem initialized successfully");
  ```

### `ostd::power`
- System control functions for rebooting, halting, and ACPI shutdown.

---

## 3. Guiding Principles When Execution Access is Restricted

When you cannot run commands directly or require user confirmation:
1. Identify the kernel domain (memory, scheduler, driver, VFS, networking).
2. Reference the exact `ostd` module above that supplies the safe abstraction.
3. Validate that safe OSTD APIs are used instead of ad-hoc `unsafe` blocks.
4. Present the user with the code solution and the exact `cargo osdk` command to test it.
