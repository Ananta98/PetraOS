---
name: osdk
description: AI skill for managing, building, running, testing, and debugging Asterinas kernels and libraries using the OSDK (Operating System Development Kit).
---

# Asterinas OSDK (Operating System Development Kit) Reference & Guide

This skill provides comprehensive instructions on how to use the OSDK (`cargo-osdk` Cargo extension) to build, run, test, and debug Rust-based operating systems (like Asterinas) and libraries.

---

## 1. Overview of OSDK

The OSDK is a command-line interface that integrates with Cargo as a subcommand (`cargo osdk`). It automates the complex steps required to cross-compile, bundle, boot, and test kernels inside virtual machine monitors (typically QEMU) and configure GDB debugging.

---

## 2. Configuration: The OSDK Manifest (`OSDK.toml`)

OSDK reads its configuration from an `OSDK.toml` file.
- **Crate-level vs. Workspace-level:** If you run an OSDK command, OSDK looks for a crate-level `OSDK.toml` first. If not found, it falls back to the workspace-level `OSDK.toml` (next to the workspace `Cargo.toml`).
- **Command & Env Substitution:** It supports shell-like command replacement to read external parameters dynamically (e.g. `kcmd_args = ["$(cat config/cmd_args)"]`).

### `OSDK.toml` Schema Reference
Here is the comprehensive configuration options available in `OSDK.toml`:

```toml
# "kernel" or "library"
project_type = "kernel"

# Supported target architectures
supported_archs = ["x86_64", "riscv64", "aarch64", "loongarch64"]

[build]
features = ["no_std", "alloc"]    # Features to enable by default
profile = "dev"                   # Cargo build profile ('dev', 'release', etc.)
strip_elf = false                 # Strip the final kernel ELF
encoding = "raw"                  # Kernel self-decompression encoding ("raw", etc.)

[boot]
method = "qemu-direct"            # Boot loader method: "qemu-direct", "grub-rescue-iso", "grub-qcow2"
kcmd_args = ["SHELL=/bin/sh"]     # Kernel command-line arguments
init_args = ["sh", "-l"]          # Init process arguments
initramfs = "path/to/initramfs"   # Custom initramfs file path

[grub]
mkrescue_path = "grub-mkrescue"   # Path to grub-mkrescue binary
boot_protocol = "multiboot2"      # Boot protocol: "linux", "multiboot", "multiboot2"
display_grub_menu = false         # Show GRUB boot menu on VM start

[qemu]
path = "qemu-system-x86_64"       # Custom path to QEMU emulator
args = "-machine q35 -m 8G -smp 1 -nographic"  # Extra QEMU command-line flags

# --- Overrides for Run/Test commands ---
[run]
# Place any [run.build], [run.boot], [run.grub], [run.qemu] overrides here

[test]
# Place any [test.build], [test.boot], [test.grub], [test.qemu] overrides here

# --- Custom Schemes ---
[scheme."custom-scheme"]
# Defines settings under '--scheme custom-scheme'
[scheme."custom-scheme".build]
profile = "release"
```

---

## 3. Core Commands and Usage

### A. Creating a New Project
```bash
cargo osdk new <name> --kernel
```
Initializes a new kernel package or library package depending on OSTD (Operating System Standard Library).

### B. Building the Crate/Kernel
```bash
cargo osdk build [OPTIONS]
```
Compiles the project, creates the initial ramdisk (if specified), and packages it into a bootable image (e.g. ISO or raw binary).
- **Key Flags:**
  - `--release`: Build in release mode.
  - `--profile <PROFILE>`: Specify custom build profile.
  - `--target-arch <ARCH>`: Choose architecture target (`x86_64`, `riscv64`, `aarch64`, `loongarch64`).
  - `--scheme <SCHEME>`: Select configuration scheme from `OSDK.toml`.
  - `--for-test`: Build kernel for unit testing.
  - `--coverage`: Enable code coverage instrumentations.

### C. Running the Crate/Kernel
```bash
cargo osdk run [OPTIONS]
```
Launches the kernel image inside QEMU.
- **Key Flags:**
  - `--scheme <SCHEME>`: Load settings of a given scheme (e.g., `tdx`, `microvm`).
  - `--kcmd-args=<ARGS>`: Override guest kernel command line parameters.
  - `--init-args=<ARGS>`: Override command line parameters passed to the init process.
  - `--qemu-args=<ARGS>`: Additional arguments to pass to QEMU.

### D. Running Kernel-Mode Tests
Unlike user-mode tests (`cargo test`), `cargo osdk test` boots the VMM, mounts the kernel-mode unit tests, and prints outcomes via the VMM console.
```bash
cargo osdk test [TESTNAME] [OPTIONS]
```
- **Key Flags:**
  - `[TESTNAME]`: Filter test cases containing this string.
  - `--coverage`: Generate test coverage reports.
  - Passes the same build options as `cargo osdk build`.

### E. Static Verification
- **Code Linting:** `cargo osdk clippy`
- **Compiler checks:** `cargo osdk check`
- **Build Docs:** `cargo osdk doc`

---

## 4. Debugging with GDB and VS Code

Debugging operating system kernels requires remote target debugging where QEMU acts as the GDB server, and a GDB client connects to it.

### Step 1: Start QEMU with GDB Server enabled
Launch the kernel using `cargo osdk run` with the `--gdb-server` option:
```bash
cargo osdk run --gdb-server addr=127.0.0.1:1234,wait-client,vscode
```
**Sub-options for `--gdb-server`:**
- `addr=ADDR`: Address/port or Unix socket on which the GDB server should listen. (Default: `.osdk-gdb-socket`).
- `wait-client`: Tells QEMU to pause kernel execution until a GDB client connects.
- `vscode`: Automatically generates a `.vscode/launch.json` file tailored for CodeLLDB/GDB inside VS Code.

### Step 2: Connect a GDB Client
#### Option A: Command Line GDB
Run the debug command to launch and connect GDB automatically to the default socket:
```bash
cargo osdk debug [OPTIONS]
```
- **Key Flags:**
  - `--remote <REMOTE>`: Remote target address (e.g., `127.0.0.1:1234` or path to Unix socket. Default: `.osdk-gdb-socket`).

#### Option B: VS Code Debugging (CodeLLDB)
1. Add the `,vscode` option to `--gdb-server` when launching the kernel.
2. In VS Code, open the Run and Debug pane (`Ctrl+Shift+D`).
3. Select the generated configuration and click "Start Debugging" (`F5`).
4. The debugger connects to the QEMU instance and starts pausing at the kernel entry point.
