# AI Agent Coding Guidelines & Behavioral Standards for PetraOS

This document defines the absolute standard of behavior, architecture, and coding practices for all AI agents collaborating on the development of **PetraOS**. PetraOS is a modular monolithic, UNIX-like Operating System written in Rust under strict `no_std` constraints for kernel space and `std` for user space.

---

## 1. LLM Behavioral Guardrails (Karpathy Guidelines)

All AI agents must strictly adhere to the following behavioral rules during development.

### 1.1 Think Before Coding
* **Zero Assumptions**: Never assume requirements, APIs, or existing structures. If any instruction is ambiguous or confusing, stop and ask the user for clarification.
* **Surface Tradeoffs**: Before implementing, state assumptions explicitly. If there are multiple design approaches, list their pros/cons and present them to the user.
* **Propose Simpler Paths**: If a requested feature or change can be done in a simpler, cleaner way, suggest it instead of blindly implementing the more complex path.

### 1.2 Simplicity First
* **Minimalist Implementations**: Write the absolute minimum code required to solve the problem. Do not add speculative features or "future-proofing" that wasn't requested.
* **No Premature Abstractions**: Do not build complex abstraction layers for single-use code.
* **No Redundant Complexity**: If a solution can be implemented in 50 lines, do not write 200 lines. Keep code concise and easy to audit.

### 1.3 Surgical Changes
* **Strict Scope Limits**: Touch only the files and lines that are strictly necessary to fulfill the task.
* **No Unsolicited Cleanup**: Do not reformat, refactor, or "fix" adjacent code that is not broken unless explicitly instructed to do so.
* **Traceability**: Every modified line must be directly traceable to the active task.

### 1.4 Goal-Driven Execution
* **Verifiable Steps**: Define clear success criteria before writing code.
* **Test-Driven Mentality**: Create tests that reproduce bugs or verify new functionality, and ensure they pass before concluding the task.

---

## 2. Directory Structure & Architecture

To maintain a clean and easily maintainable codebase, the modular monolithic architecture of PetraOS must organize its kernel space into the following directories:

```
petraos/
├── kernel/
│   ├── src/
│   ├── arch/       # CPU architecture-specific code (e.g., x86_64, aarch64)
|   ├── fs/         # File systems (e.g., FAT, NTFS, ext4)
|   ├── ipc/        # Inter-process communication (e.g., pipes, message queues, shared memory)
│   ├── drivers/    # Hardware device drivers (e.g., serial, vga, timer, keyboard)
│   ├── sched/      # Process scheduling algorithms (e.g., Round Robin, Priority, CFS)
│   ├── syscalls/   # Kernel system call interface and handlers
│   ├── proc/       # Process and thread management (PCBs, context switching)
│   ├── sync/       # Synchronization primitives (e.g., mutexes, semaphores, condition variables)
│   ├── net/        # Networking stack (e.g., TCP/IP, UDP, Ethernet)
│   ├── mm/         # Memory management (physical, virtual, heap allocator)
│   ├── limine.rs   # Limine protocol variables related
|   ├── logger.rs   # Logging related
│   └── main.rs     # Main entry for Petra operating system
├── userspace/      # User-space libraries, runtimes, and applications
└── tests/          # Intensive kernel-mode and user-mode test suites
```

---

## 3. Rust `no_std` Kernel Development Rules

PetraOS is a pure `#![no_std]` and `#![no_main]` environment. The following rules are non-negotiable:

* **Zero Third-Party Dependencies**: No external crates or third-party dependencies are allowed unless explicitly requested and approved by the user. Rely only on the Rust `core` and `alloc` libraries.
* **Memory Safety & Unsafe Code**:
  * Keep `unsafe` blocks as small as possible.
  * Every `unsafe` block must be preceded by a `// SAFETY:` comment explaining why the operation is guaranteed to be safe under the current invariants.
  * Encapsulate raw pointers and hardware interaction behind safe Rust abstractions.
* **Panic Avoidance**:
  * Never use `.unwrap()`, `.expect()`, or index operations that could panic (`slice[index]`) unless it is mathematically proven to be impossible to fail, in which case it must be documented.
  * Use `Result` and `Option` propagation (`?` operator) extensively.
  * Implement clean error types for memory allocation failures, hardware timeouts, and system limits.

---

## 4. Rust Abstractions & Generics for Extensibility

To support clean extensibility across different CPU architectures and diverse device drivers, agents must utilize Rust traits and generics.

### 4.1 Architecture Abstraction Layer (AAL)
Avoid hardcoding architecture-specific instructions (like `outb` or assembly context switches) in the core kernel logic. Instead, define clean traits in common modules:

```rust
pub trait CpuArch {
    /// Disable interrupts on the current core and return the previous state.
    fn disable_interrupts() -> bool;
    
    /// Enable interrupts on the current core.
    fn enable_interrupts();
    
    /// Halt the CPU until the next interrupt.
    fn halt();
    
    /// Initialize architecture-specific tables (GDT, IDT/IVT, Page Tables).
    fn init_hardware();
}
```

### 4.2 Generic Driver Interface
Device drivers must implement common traits to allow modular attachment and generic handling by the kernel:

```rust
pub trait DeviceDriver {
    /// Return the user-friendly name of the device.
    fn name(&self) -> &'static str;
    
    /// Initialize the hardware device.
    fn init(&mut self) -> Result<(), DriverError>;
}

pub trait CharDevice: DeviceDriver {
    /// Read a single byte from the character device.
    fn read_byte(&mut self) -> Result<u8, DriverError>;
    
    /// Write a single byte to the character device.
    fn write_byte(&mut self, byte: u8) -> Result<(), DriverError>;
}

pub trait BlockDevice: DeviceDriver {
    /// Read sectors from the block device into the buffer.
    fn read_blocks(&self, start_sector: u64, buf: &mut [u8]) -> Result<(), DriverError>;
    
    /// Write sectors from the buffer to the block device.
    fn write_blocks(&self, start_sector: u64, buf: &[u8]) -> Result<(), DriverError>;
}
```

---

## 5. Readability & Code Quality Standards

Code quality must be production-ready. Readability is paramount.

### 5.1 Naming Conventions
* **Types and Traits**: `PascalCase` (e.g., `ProcessControlBlock`, `VirtualMemoryManager`).
* **Functions, Variables, Modules**: `snake_case` (e.g., `allocate_page`, `bytes_written`).
* **Constants and Statics**: `SCREAMING_SNAKE_CASE` (e.g., `KERNEL_BASE_ADDRESS`).
* **Generics**: Clear uppercase identifiers or descriptive names (e.g., `T`, `D: DeviceDriver`).
* **Variable Intention**: Name variables by what they hold, not their type (e.g., use `allocated_pages` instead of `page_list_vec`).

### 5.2 Control Flow & Idiomatic Rust
* **Guard Clauses**: Use guard clauses and early returns to minimize nested structures.
  ```rust
  // PREFERRED
  if count == 0 {
      return Ok(());
  }
  // Proceed with logic...
  ```
* **Expressive Pattern Matching**: Use `match` or `if let` blocks rather than deep chain logic.
* **Type-Driven Safety**: Leverage Rust's type system to enforce constraints (e.g., wrapping raw addresses in `PhysicalAddress` and `VirtualAddress` structs so they cannot be mixed up).

### 5.3 Modularity & Module Splitting
* **File Size & Separation**: If a module grows large (e.g., exceeding ~300-500 lines) or handles multiple distinct responsibilities (e.g., driver initialization vs. I/O processing vs. interrupt handling), split it into submodules and individual files.
* **Avoid Monolithic Files**: Do not dump all subsystem logic inside a single file (like `mod.rs` or `lib.rs`). Use Rust's module hierarchy and subdirectory layout to split components cleanly (e.g., `arch/x86_64/gdt.rs` and `arch/x86_64/idt.rs` instead of one giant file).
* **Clean Encapsulation & APIs**: Expose only the necessary minimal interface using `pub` or `pub(crate)`. Keep private implementation details hidden to facilitate easier debugging, testing, and isolated modifications.

---

## 6. User-Space Coding Rules

User-space applications run in unprivileged CPU modes and interact with the kernel strictly via system calls.

* **System Call Layer**: Wrap system calls in a standard library wrapper (`no_std`) providing safe Rust functions that return `Result<T, SystemError>`.
* **Zero Shared Memory (by default)**: Enforce address space isolation. Memory is shared between user space and kernel space only through explicit memory mapping operations (`mmap`) or safe buffer validations in syscall handlers.
* **Resource Cleanup**: User-space applications must be written to cleanly release resources, closing descriptors and freeing heap space. The kernel must ensure complete cleanup of a process's page directory and allocated structures on termination.

---

## 7. Intensive Testing Protocol

All code modifications are subject to rigorous verification.

* **Write Tests First/Simultaneously**: For every task or bug fix, write corresponding tests in the `tests/` directory.
* **Kernel-Mode Testing**: Test kernel modules (e.g., memory allocator, scheduler, page table mapping) using unit tests in a simulated or mock environment, or integration tests inside QEMU.
* **User-Mode Testing**: Verify system calls and process isolation using integration tests that execute specific user-space binaries and assert expected return codes and outputs.
* **No Unverified Commits**: Do not consider a task complete until all tests run successfully and show zero regressions.

---

## 8. QEMU Testing & Execution Guidelines

All agents must verify kernel-level changes by running and testing them in QEMU.

### 8.1 How to Run QEMU Properly
* **Headless/Serial Mode**: If you are executing in a headless environment, command-line sandbox, or terminal without GUI forwarding, run QEMU with the graphics display disabled and the serial output redirected to standard output. Override `QEMUFLAGS` to achieve this:
  ```bash
  make run QEMUFLAGS="-m 2G -display none -serial stdio"
  ```
  Or for BIOS boot:
  ```bash
  make run-bios QEMUFLAGS="-m 2G -display none -serial stdio"
  ```
* **Monitoring Output**: Pay close attention to standard output. Verify that the bootloader (Limine) successfully loads the kernel, that the kernel initializes, and that no panic occurs.
* **Graceful Termination**: QEMU runs indefinitely until interrupted or exited via debug ports. Once you have verified the serial logs and confirmed successful execution, terminate the QEMU process. In the terminal or background task manager, send a SIGINT (`Ctrl+C`) or kill the process. Do not let orphaned QEMU processes run in the background.

### 8.2 Declaring QEMU Test Results
* **Required Test Statement**: Once testing is complete, you **must** explicitly report the status of the QEMU test in your final response or walkthrough.
* **Format**: Include a clear declaration in your final response under a dedicated heading:
  > **QEMU Test Verification**:
  > * **Command Run**: `make run QEMUFLAGS="..."`
  > * **Result**: Success / Failure / Panic (with a brief description of what was verified, e.g., "Serial logs successfully displayed GDT and IDT initialization messages without panic").

---

## 9. Git Version Control & Commit Integrity

To preserve ownership and trace project accountability:
* **Configured Git Author Only**: All commits must be authored strictly using the system's configured Git identity.
  * Do **NOT** override `git config user.name` or `git config user.email` inside shell commands or configuration scripts.
  * Let Git naturally use the user's configuration (`git config --global user.name` / `user.email`).
* **Surgical Commits**: Commit messages must be clear, descriptive, and reference the specific subsystem modified (e.g., `mm: implement buddy allocator initialization`).
* **Single Change per Commit**: Keep commits modular. Do not bundle unrelated modifications together.

