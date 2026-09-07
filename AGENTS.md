# AI Agent Coding Guidelines & Behavioral Standards for PetraOS

PetraOS is a modular monolithic, UNIX-like OS written in Rust — `no_std` / `no_main` in kernel space, `std` in user space.

## 0. Mandatory Startup Requirement

**Every agent must read `AGENTS.md` at the start of every session/task** before inspecting code, planning, or making edits. Do not rely on cached knowledge — re-read this file to ensure compliance with current architecture, style, and workflow rules.

---

## 1. Git Authorship & Attribution (Strict Human Attribution)

* **Human committer only:** All commits, patch headers (`From:`), `bootstrap.yml` / `packages/**/*.yml` (`patch_author`, `patch_email`), and module metadata must use the human maintainer from `git config user.name` / `user.email`. Never use AI identities.
* **No AI author:** Never use `AI Agent`, `PetraOS Agent`, `Antigravity`, `ChatGPT`, `Claude`, or synthetic emails like `agent@petraos.dev`.
* **File headers:** `@author`, module creator, and doc author tags must attribute the human developer, not an AI.

## 2. Directory Structure & Architecture

### 2.1 Repository Layout

```
petraos/
├── kernel/          # Rust kernel crate (no_std, no_main)
├── mlibc/           # C standard library port (sysdeps/petra)
├── packages/        # Userland ports (xbstrap)
├── cross-files/     # Meson cross-compilation files
├── tools/           # Build scripts, initramfs generator
├── limine/          # Bootloader assets (+ limine.conf)
├── bootstrap.yml    # xbstrap orchestration manifest
└── GNUmakefile      # Top-level build (kernel / ISO / initramfs / QEMU)
```

### 2.2 Kernel Subsystems (`kernel/src/`)

```
kernel/src/
├── arch/        # Architecture-specific (x86_64, aarch64) — GDT/IDT, paging, interrupts
├── device/      # Unified device model & driver traits
├── drivers/     # Hardware drivers (block, bus, char, gpu, net, time)
├── fs/          # VFS and filesystems (ext2, devfs, ramfs, pipe, initramfs)
├── ipc/         # IPC & signals
├── mm/          # Memory management (pmm, vmm, alloc, types)
├── modules/     # Module system & initcall levels
├── net/         # Network stack
├── proc/        # Process & thread management, ELF loader
├── sched/       # Scheduler (CFS, nice)
├── security/    # Credentials, UID/GID, capabilities
├── sync/        # Synchronization primitives (mutex, rwlock, futex)
├── syscalls/    # Syscall dispatcher & handlers
├── utils/       # Helpers (e.g., CPIO)
├── limine.rs    # Boot protocol structures
├── logger.rs    # Kernel console / serial logger
└── main.rs      # Kernel entry (kmain)
```

Keep this overview at directory level. Refer to `kernel/src/<subsystem>/mod.rs` for file-level detail when needed.

## 3. Rust `no_std` Kernel Rules

* **No third-party crates** unless explicitly approved. Use only `core` and `alloc`.
* **Unsafe:** Keep blocks minimal. Every `unsafe` requires a preceding `// SAFETY:` comment explaining invariants. Wrap raw pointers / MMIO behind safe abstractions.
* **No panics:** Avoid `.unwrap()`, `.expect()`, and panicking indexing. Use `Result`/`Option` with `?`. Define explicit error types for allocation, timeout, and limit failures. Document any proven-infallible unwrap.

## 4. Rust Abstractions & Generics

Use traits and generics for extensibility — especially for drivers and subsystems.

```rust
pub trait DeviceDriver {
    fn name(&self) -> &'static str;
    fn init(&mut self) -> Result<(), DriverError>;
}
pub trait CharDevice: DeviceDriver {
    fn read_byte(&mut self) -> Result<u8, DriverError>;
    fn write_byte(&mut self, byte: u8) -> Result<(), DriverError>;
}
pub trait BlockDevice: DeviceDriver {
    fn read_blocks(&self, start_sector: u64, buf: &mut [u8]) -> Result<(), DriverError>;
    fn write_blocks(&self, start_sector: u64, buf: &[u8]) -> Result<(), DriverError>;
}
```

Prefer generic bounds (`D: DeviceDriver`) over duplicated per-device logic.

## 5. Code Reusability

* **DRY first:** Before writing new code, search for existing helpers, traits, or utils that already solve the problem. Reuse — don't duplicate.
* **Extract common logic:** Shared behavior across drivers/subsystems goes into `device/`, `utils/`, `mm/types/`, `sync/`, or a common trait — not copy-pasted per caller.
* **Traits over duplication:** Model variations via traits and generics, not cloned implementations.
* **Small, reusable units:** Keep functions focused and side-effect-free where possible; prefer composable helpers over monolithic routines.
* **Single source of truth:** Centralize constants, addresses, and protocol definitions. Import them — don't redefine.
* **Reusability check on review:** If new code duplicates >2 lines of existing logic, refactor into a shared function/module first.

## 6. Readability & Code Quality

**Naming:** `PascalCase` types/traits, `snake_case` functions/vars/modules, `SCREAMING_SNAKE_CASE` constants, descriptive generics (`T`, `D: DeviceDriver`), intent-revealing names (`allocated_pages` not `page_list_vec`).

**Control flow:** Use guard clauses / early returns and `match` / `if let` over deep nesting. Leverage the type system (`PhysicalAddress` vs `VirtualAddress`) to enforce invariants.

**Modularity:** Split files exceeding ~300-500 lines or mixing responsibilities into submodules (e.g., `arch/x86_64/gdt.rs` + `idt.rs`). Expose minimal `pub`/`pub(crate)` surface; keep internals private.

## 7. Text Search & Indexing

Before sequential searches (`grep`/`rg`/`find`/`ls`), consult the structural knowledge graph:

1. `/graphify query "<intent>"` — find hubs
2. `/graphify path "<A>" "<B>"` — trace dependencies
3. Fall back to `grep` only for exact known symbols
