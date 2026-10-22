# AI Agent Guidelines for PetraOS

This document defines architectural standards, coding principles, and the mandatory review workflow for AI agents modifying PetraOS.

---

## 1. Project Overview

- **Kernel**: `silica` — modular Rust operating system kernel.
- **Framework**: Built on OSTD (Operating System Standard Library) using Asterinas OSDK (`cargo osdk`).
- **Toolchain**: Rust nightly, 2024 edition. Binaries are in `~/.cargo/bin`.

---

## 2. Architecture & File Structure

Code in `silica/src/` is strictly partitioned into domain-specific subsystems:

| Directory | Subsystem / Responsibility |
| :--- | :--- |
| `arch/` | Architecture-specific code (e.g., `x86_64`) |
| `uapi/` | Userspace ABI / POSIX structures & error codes |
| `drivers/` | Device drivers (`block`, `char`, `net`, `bus`, `drm`) |
| `fs/` | Virtual File System (`vfs`), `devfs`, `procfs`, `sysfs`, `pipe`, `ext2`, `exfat` |
| `ipc/` | IPC primitives (`shm`, `sem`, `msg`, `perm`) |
| `net/` | Network stack and sockets (`smoltcp`, `unix`, `netlink`) |
| `proc/` | Process lifecycle, threads, scheduler hooks, `fork`, `exec`, `signal` |
| `sched/` | Scheduling algorithms and adapter layer |
| `syscall/` | System call dispatchers and handlers |
| `time/` | Clock sources, TSC, RTC, timers |
| `vm/` | Virtual memory address spaces (`vmar`), backing files, paging |
| `utils/` | Low-level data structures (e.g., lockless queues, cpio) |

### Placement Rules
- Place new files in the corresponding subsystem directory. Never dump random utilities into top-level or unrelated directories.
- Register new files in the local subsystem `mod.rs`. Expose only what other modules legitimately need using `pub(crate)` or trait interfaces.

---

## 3. Design Principles

### Simplicity & Readability
- **Write straightforward Rust**: Favor clear, explicit code over clever type gymnastics, premature macros, or unnecessary generics.
- **Self-documenting**: Use clear, descriptive names for types, functions, and variables.
- **Concise comments**: Explain *why* a design decision was made, not what the code already says. Avoid redundant commentary.
- **No dead code**: Remove unused imports, dead functions, and temporary debug prints before concluding work.

### Clean Abstraction & Low Coupling
- **Interface boundaries**: Subsystems must interact through explicit traits or minimal functional APIs, not by accessing raw internal state of other subsystems.
- **No circular dependencies**: Keep data flow unidirectional and hierarchical.
- **Avoid over-abstraction**: Abstract only when multiple concrete implementations exist or when needed to isolate hardware/subsystem boundaries.

---

## 4. Mandatory Post-Change Workflow

Whenever an agent modifies the codebase, it **MUST** perform the following steps before finishing the task:

1. **Review Diff**:
   - Inspect changes (`git diff`).
   - Check file placement, module hygiene, and clean abstraction boundaries.
   - Refactor messy shortcuts, dead code, or unnecessary complexity.
   - Activate the `code-review` skill (`.agents/skills/code-review/SKILL.md`) for systematic audit.

2. **Verify Build & Run**:
   - Run compilation and kernel build check:
     ```bash
     export PATH="$HOME/.cargo/bin:$PATH"
     cargo osdk check
     cargo osdk build
     ```
   - Alternatively run the verification helper:
     ```bash
     bash .agents/skills/code-review/scripts/verify.sh
     ```
   - **Never leave a broken build.** The task is only complete when `cargo osdk check` and `cargo osdk build` pass with exit code `0`.

---

## 5. Available Skills & OSTD Routing Policy

The following project skills are available in `.agents/skills/`:
- **`code-review`** (`.agents/skills/code-review/SKILL.md`): Systematic audit of file structure, readability, loose coupling, and build verification.
- **`osdk`** (`.agents/skills/osdk/SKILL.md`): Build, run, test, and debug the kernel via Asterinas OSDK.

### Access Restrictions & User Approval Routing
If an agent lacks terminal execution permissions, is denied access, or requires explicit user approval for intrusive or long-running commands (e.g., booting QEMU via `cargo osdk run`, interactive debugging, or modifying VM configurations):
- **Do NOT attempt unauthorized execution or stall.**
- **Route to OSTD documentation**:
  1. Consult and reference the OSTD guide ([`.agents/skills/osdk/references/ostd.md`](.agents/skills/osdk/references/ostd.md)) or official documentation ([https://asterinas.github.io/api-docs](https://asterinas.github.io/api-docs)).
  2. Ground solutions in safe OSTD subsystem primitives (`ostd::mm`, `ostd::task`, `ostd::sync`, `ostd::irq`, `ostd::io`, `ostd::user`).
  3. Provide the user with the recommended code changes and exact manual `cargo osdk` commands to run.
