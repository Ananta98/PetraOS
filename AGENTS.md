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

### 1.5 Git Authorship, Patch Metadata & Committer Policy (Strict Human Attribution)
* **Human Git Committer Identity**: All git commits, patch headers, package manifests, and module metadata must attribute the human git committer / repository maintainer (from `git config user.name` and `git config user.email`), **NEVER** an AI Agent.
* **No AI Patch Author / Email**: In patch series (`From: ...`), xbstrap configurations (`bootstrap.yml`, `packages/**/*.yml` with `patch_author` and `patch_email`), never use names like `"AI Agent"`, `"PetraOS Agent"`, `"Antigravity"`, `"ChatGPT"`, `"Claude"`, or synthetic bot emails (e.g., `agent@petraos.dev`). Always use the human committer's name and email.
* **Module Creator & File Headers**: Any source file headers, module creator tags (`@author`), or documentation author fields must specify the human developer/maintainer as the creator, not an AI persona.

---

## 2. Directory Structure & Architecture

To maintain a clean and easily maintainable codebase, the modular monolithic architecture of PetraOS organizes the repository and kernel space into the following directory layout:

### 2.1 Repository Structure
```
petraos/
├── kernel/             # Rust kernel crate (#![no_std], #![no_main])
│   ├── src/            # Kernel source code (modular subsystems)
│   ├── Cargo.toml      # Kernel cargo manifest
│   ├── build.rs        # Kernel build script
│   ├── linker-*.ld     # Architecture-specific linker scripts (x86_64, aarch64)
│   └── GNUmakefile     # Kernel build rules
├── mlibc/              # C standard library port (sysdeps/petra)
├── packages/           # Userland package definitions & patches (xbstrap ports)
│   ├── bash/           # GNU Bash port & recipe
│   ├── gcc/            # GCC toolchain recipe
│   ├── mlibc/          # mlibc package manifest & port patches
│   └── readline/       # Readline port & recipe
├── cross-files/        # Meson cross-compilation configurations for targets
├── tools/              # Build scripts, initramfs generator (create_initramfs.sh)
├── initramfs_root/     # Staging root filesystem directory for initramfs
├── limine/             # Limine bootloader binary assets
├── limine.conf         # Limine bootloader configuration
├── bootstrap.yml       # xbstrap build orchestration manifest
└── GNUmakefile         # Top-level orchestrator for kernel, ISO, initramfs & QEMU
```

### 2.2 Kernel Subsystem Architecture (`kernel/src/`)
```
kernel/src/
├── arch/               # CPU architecture-specific code (x86_64, aarch64)
│   └── x86_64/         # GDT, IDT, ACPI, CPU state, interrupts, paging, signals, syscall entry, timer
├── device/             # Unified device model, bus management, driver traits & device manager
├── drivers/            # Hardware device drivers by category
│   ├── block/          # Block storage drivers (AHCI, NVMe, etc.)
│   ├── bus/            # Bus drivers (PCI, PCIe, etc.)
│   ├── char/           # Character devices (serial 16550 UART, console, etc.)
│   ├── gpu/            # Framebuffer & display drivers
│   ├── net/            # Network interface card drivers
│   └── time/           # Hardware timers (RTC, PIT, HPET, APIC timer)
├── fs/                 # Virtual File System (VFS) & concrete/pseudo filesystems
│   ├── vfs/            # VFS core, inode, dentry, mount points, file operations
│   ├── devfs.rs        # Device filesystem (/dev)
│   ├── ext2/           # Ext2 filesystem driver
│   ├── fd.rs           # Process file descriptor table management
│   ├── initramfs.rs    # CPIO initramfs loader and mounting
│   ├── ioctl.rs        # IOCTL dispatcher and device control handlers
│   ├── pipe.rs         # Anonymous and named UNIX pipe implementation
│   └── ramfs.rs        # In-memory RAM filesystem
├── ipc/                # Inter-process communication
│   ├── signal.rs       # POSIX signal delivery, handling & sigaction
│   └── ...             # Pipes, queues, shared memory primitives
├── mm/                 # Memory management subsystem
│   ├── alloc/          # Dynamic heap allocator (slab, buddy, bump)
│   ├── pmm/            # Physical Memory Manager (page frame allocator, bitmap/buddy)
│   ├── types/          # Strongly-typed PhysicalAddress, VirtualAddress, Page, Frame
│   └── vmm/            # Virtual Memory Manager (page tables, address space, mmap)
├── modules/            # Kernel module & extension subsystem
│   ├── initcall.rs     # Level-based initcall mechanism (early, core, driver, late)
│   ├── manager.rs      # Dynamic module lifecycle manager
│   └── module.rs       # Module metadata, registration traits & descriptors
├── net/                # Network subsystem and protocol stack (sockets, TCP/IP, UDP, Ethernet)
├── proc/               # Process and thread management
│   ├── loader/         # ELF64 binary loader and auxiliary vector setup
│   ├── process/        # Process Control Block (PCB), PID allocation, process hierarchy
│   └── thread/         # Thread Control Block (TCB), kernel/user threads, context switching
├── sched/              # Process scheduling subsystem
│   ├── fair.rs         # Completely Fair Scheduler (CFS) implementation
│   ├── nice.rs         # POSIX nice levels & dynamic weight calculations
│   └── mod.rs          # Scheduler core, runqueues, context switch dispatcher
├── security/           # Access control, credentials, UID/GID, POSIX capabilities
├── sync/               # Kernel synchronization primitives
│   ├── futex.rs        # Fast Userspace Mutex (futex) wait/wake implementation
│   ├── mutex.rs        # Kernel blocking mutex
│   ├── rwlock.rs       # Read-Write Lock
│   └── spinlock.rs     # Ticket / atomic spinlock
├── syscalls/           # System call dispatcher and handler implementations
│   ├── arch_prctl.rs   # Architecture-specific process control (FS_BASE/GS_BASE)
│   ├── fs.rs           # Filesystem syscalls (open, read, write, close, stat, etc.)
│   ├── ioctl.rs        # Device ioctl syscall handler
│   ├── mm.rs           # Memory syscalls (mmap, munmap, mprotect, brk)
│   ├── proc.rs         # Process syscalls (fork, execve, exit, wait4, getpid)
│   ├── signals.rs      # Signal syscalls (sigaction, kill, sigprocmask)
│   ├── sync.rs         # Synchronization syscalls (futex)
│   ├── sys_info.rs     # System information syscalls (uname, sysinfo)
│   ├── time.rs         # Time syscalls (clock_gettime, nanosleep)
│   └── mod.rs          # Central syscall dispatcher table and argument decoding
├── utils/              # Utility helpers
│   ├── cpio.rs         # CPIO archive unpacker for initramfs
│   └── mod.rs          # General utility functions
├── limine.rs           # Limine boot protocol request structures & boot info
├── logger.rs           # Serial port logger & kernel console output
└── main.rs             # Kernel entry point (`kmain`) and initialization sequence
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

To support clean extensibility across different diverse device drivers, agents must utilize Rust traits and generics.

### 4.1 Generic Driver Interface
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

## 7. QEMU Testing & Execution Guidelines

All agents must verify kernel-level changes by running and testing them in QEMU.

### 7.1 How to Run QEMU Properly
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

### 7.2 Declaring QEMU Test Results
* **Required Test Statement**: Once testing is complete, you **must** explicitly report the status of the QEMU test in your final response or walkthrough.
* **Format**: Include a clear declaration in your final response under a dedicated heading:
  > **QEMU Test Verification**:
  > * **Command Run**: `make run QEMUFLAGS="..."`
  > * **Result**: Success / Failure / Panic (with a brief description of what was verified, e.g., "Serial logs successfully displayed GDT and IDT initialization messages without panic").