# PetraOS

PetraOS is a modular monolithic, UNIX-like operating system written in Rust (`no_std` kernel) featuring a POSIX userland powered by mlibc, GNU toolchains, and xbstrap package orchestration.

---

## 1. Prerequisites

### Host System Tools
Ensure the required build tools and xbstrap are installed:
```bash
# Ubuntu / Debian
sudo apt update
sudo apt install -y build-essential git cpio xorriso gdisk mtools qemu-system-x86 python3 python3-pip curl ninja-build meson cmake
pip install xbstrap || pip install --break-system-packages xbstrap

# Rust bare-metal target for kernel
rustup target add x86_64-unknown-none
```

---

## 2. Quick Start

### Build Everything & Run in QEMU
To build the complete userspace, kernel, initramfs, and launch PetraOS in QEMU:
```bash
make build-userspace
make run
```

---

## 3. Userspace Build System

All package recipes reside under `packages/<package>/<package>.yml`. Userspace compilation uses a two-phase bootstrapping architecture managed via `GNUmakefile` and `tools/build_userland.sh`:

| Command | Action |
| :--- | :--- |
| `make build-userspace` | Full pipeline: builds host tools & toolchain, then builds all target packages into the sysroot |
| `make build-tools` | Phase 1: Builds host cross-compiler tools (`host-binutils`, `host-gcc`, `mlibc-headers`, `mlibc`) |
| `make build-packages` | Phase 2: Compiles all target userspace packages (`bash`, `coreutils`, `ncurses`, `vim`, etc.) |
| `make fetch-userspace` | Downloads all package sources to `sources/` without compiling |
| `make clean-userspace` | Completely resets the workspace (`build-xbstrap` and `sources/`) for a clean slate build |

### Single Package Operations
You can also compile or inspect individual packages via `tools/build_userland.sh`:
```bash
./tools/build_userland.sh build <pkg>    # Build single package into sysroot
./tools/build_userland.sh clean <pkg>    # Clean build cache for single package
./tools/build_userland.sh status [pkg]   # Check build/download status
```

---

## 4. Kernel & Initramfs

```bash
make -C kernel       # Build the Rust kernel binary
make initramfs       # Sync sysroot and package build/initramfs.cpio
make run             # Boot ISO in QEMU (4GB RAM, serial stdio)
```
