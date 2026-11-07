# PetraOS Userland Build & Packaging Guide

This document describes the userspace architecture, package provisioning workflow, and rootfs image generation for **PetraOS**.

---

## 1. Overview

PetraOS uses a hybrid two-tier userspace model based on **Void Linux rootfs (x86_64, glibc)** with prebuilt package layers:

1. **In-Memory Boot Initramfs (`build/initramfs.cpio`)**:
   - Lightweight CPIO archive containing the PID 1 `/init` script, core utilities, dynamic linker (`ld-linux-x86-64.so.2`), and recovery shell fallback.
   - Mounts pseudo-filesystems (`/proc`, `/sys`, `/dev`, `/run`), detects block storage (`/dev/sda` / `/dev/vda`), mounts the ext4 rootfs to `/sysroot`, prepares nested devpts/sysfs/procfs, and transitions into the full Void Linux rootfs environment.

2. **Full Rootfs Block Image (`build/ext4.img`)**:
   - Complete ext4 filesystem image populated with standard Void Linux glibc rootfs base, and prebuilt `developer-tools` layers (`bash`, `python3`, `btop`, `e2fsprogs`, `exfatprogs`, GCC/binutils, Make, CMake, Git, GDB, strace, etc.).

---

## 2. Rootfs Packages & Repositories

The userland build script (`scripts/build-userland/build-userland-amd64.sh`) uses prebuilt package scripts in `scripts/prebuild-package/` and official static XBPS (`xbps-install.static`) to provision packages without requiring host root privileges.

### Configured Repositories
- Void Linux Current (`https://repo-default.voidlinux.org/current`)

### Installed Package Layers
- `developer-tools`: `bash`, `python3`, `btop`, `e2fsprogs`, `exfatprogs`, `gcc`, `binutils`, `make`, `cmake`, `meson`, `ninja`, `git`, `gdb`, `strace`, `htop`, `tmux`, `vim`, etc.

---

## 3. Building the Userland

### Prerequisites
The host system requires (Linux / Ubuntu / WSL):
- `bash`, `curl`, `tar`, `xz`, `file`, `cpio`, `mke2fs` (`e2fsprogs`)
- Internet connectivity to fetch Void Linux rootfs and package archives.

### Running the Builder
Run the userland assembly script from the repository root:

```bash
# Assembles the Void-based userland, staging directories, and images
bash scripts/build-userland/build-userland-amd64.sh
```

### Useful Options:
- `--fresh`: Cleans staging directories and rebuilds userland from scratch.
- `--purge-cache`: Combined with `--fresh`, removes cached tarballs and packages before re-downloading.

---

## 4. Packing Initramfs & Filesystem Images

If you modify staging directories directly, you can repack without re-downloading packages:

```bash
bash scripts/build-initramfs.sh
```
