---
name: osdk
description: AI skill for managing, building, running, testing, and debugging Asterinas/PetraOS kernels using OSDK, with mandatory routing to OSTD documentation when execution access is restricted or requires user approval.
---

# Asterinas OSDK (Operating System Development Kit) Skill

This skill guides agents in using the Asterinas OSDK (`cargo osdk`) to build, test, run, and debug the PetraOS `silica` kernel, and defines the mandatory routing to OSTD documentation when execution permissions or user approvals are required.

---

## 1. Environment Setup

Always ensure Cargo and Rust tools are available in `PATH`:
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

---

## 2. Core OSDK Workflows

### A. Checking & Compilation
- **Quick type check:**
  ```bash
  cargo osdk check
  ```
- **Build kernel binary:**
  ```bash
  cargo osdk build
  ```
- **Build in release mode:**
  ```bash
  cargo osdk build --release
  ```

### B. Running in Virtual Machine (QEMU)
```bash
cargo osdk run [OPTIONS]
```
Common options:
- `--kcmd-args="<ARGS>"`: Override kernel command line parameters.
- `--init-args="<ARGS>"`: Override arguments passed to userspace init.
- `--qemu-args="<ARGS>"`: Pass extra flags to QEMU.

Configuration defaults are specified in [`OSDK.toml`](file:///home/ananta/PetraOS/OSDK.toml).

### C. Kernel-Mode Testing
Unlike standard user-space `cargo test`, `cargo osdk test` boots into QEMU and executes kernel unit tests:
```bash
cargo osdk test [TESTNAME]
```

### D. Remote GDB Debugging
1. Start QEMU with GDB server enabled:
   ```bash
   cargo osdk run --gdb-server addr=127.0.0.1:1234,wait-client
   ```
2. In another terminal, connect GDB:
   ```bash
   cargo osdk debug --remote 127.0.0.1:1234
   ```

### E. Generating Documentation
```bash
cargo osdk doc
```
Generates HTML documentation for both the `silica` kernel and the underlying `ostd` framework in `target/osdk/doc/`.

---

## 3. Mandatory Protocol: Access Restrictions & User Approval

> [!IMPORTANT]
> If an agent **does not get access** from the user, lacks terminal execution privileges, or an action **requires user approval** (e.g., launching full QEMU instances via `cargo osdk run`, invasive debugging, modifying boot images, or running commands where permission is withheld):
>
> **The agent MUST route to OSTD documentation rather than halting or attempting unauthorized execution.**

### Routing Actions:
1. **Consult & Cite OSTD Documentation**:
   - **Local Reference Guide**: Direct the user and reference [OSTD Architecture Reference](./references/ostd.md).
   - **Local Rustdoc**: Point to local docs generated via `cargo osdk doc` (located at `target/osdk/doc/ostd/index.html`).
   - **Local Crate Source**: Inspect standard implementations directly in `~/.cargo/registry/src/*ostd-0.18.1/src/`.
   - **Online Official Documentation**:
     - OSTD API Docs: `https://asterinas.github.io/api-docs`
     - Crate Documentation: `https://docs.rs/ostd/0.18.1`
     - Asterinas Book: `https://asterinas.github.io/book`

2. **Map Kernel Subsystems to Safe OSTD Abstractions**:
   When answering design questions or proposing implementations without live execution access, ground the solution strictly in OSTD's safe primitives:
   - **Memory Management**: `ostd::mm` (`VMRange`, `PageTable`, `PAddr`, `VAddr`, `Frame`, `allocator_api`).
   - **Tasks & Concurrency**: `ostd::task` (thread lifecycle, task states) and `ostd::sync` (`SpinLock`, `Mutex`, `RwLock`, `WaitQueue`).
   - **Hardware & Interrupts**: `ostd::irq` (interrupt handlers, disable/enable local IRQ), `ostd::io` (`IoPort`, `Mmio`), and `ostd::bus` (PCI bus driver interfaces).
   - **CPU & Timing**: `ostd::cpu` (per-CPU data, local APIC), `ostd::smp`, `ostd::timer` (monotonic timers, calibration), and `ostd::power` (system reset, ACPI shutdown).
   - **Userspace Isolation**: `ostd::user` (safe user memory validation, pointer access, syscall entry/exit).
   - **Logging**: `ostd::log` and `ostd::console` (`info!`, `warn!`, `error!`, `debug!`).

3. **Provide Manual Steps for the User**:
   Clearly supply the exact code changes and terminal commands so the user can review and execute them at their own discretion.
