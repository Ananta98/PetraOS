# PetraOS

A modern, modular operating system kernel written in Rust, built on OSTD (Operating System Standard Library) using Asterinas OSDK (`cargo osdk`).

---

## Features

- **Subsystem Architecture**: Domain-partitioned kernel subsystems (`arch`, `fs`, `drivers`, `ipc`, `net`, `proc`, `sched`, `syscall`, `vm`, `time`).
- **Linux ABI Compatibility**: Supports Rocky Linux 9 userland binaries, GNU toolchain (`gcc`, `g++`, `gdb`, `make`), and OpenRC.
- **VFS Subsystem**: Full virtual filesystem supporting `ext4`, `exfat`, `procfs`, `sysfs`, `devfs`, and `tmpfs`.
- **Networking**: `AF_INET`, `AF_UNIX` (stream and datagram with `SCM_RIGHTS` & `SCM_CREDENTIALS`), and `AF_NETLINK` route socket support.

---

## Quick Start

### 1. Build the Kernel

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo osdk check
cargo osdk build
```

### 2. Build Userland & Filesystem Images

Refer to the [Userland Guide](docs/USERLAND.md) for full instructions:

```bash
# Provision Rocky Linux 9 rootfs, EPEL repository, fastfetch, and pack images
bash scripts/build-userland/build-userland-amd64.sh
```

### 3. Run under QEMU

```bash
cargo osdk run
```

---

## Documentation

- [Userland & EPEL Guide](docs/USERLAND.md): Packaging and provisioning Rocky Linux 9 userland, `epel-release`, and `fastfetch`.
- [Agent Guidelines](AGENTS.md): Architecture standards, coding principles, and human committer rules.

