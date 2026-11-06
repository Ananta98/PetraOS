# PetraOS Userland Build & Packaging Guide

This document describes the userspace architecture, package provisioning workflow, and rootfs image generation for **PetraOS**.

---

## 1. Overview

PetraOS uses a hybrid two-tier userspace model based on **Alpine Linux minirootfs (x86_64)** with prebuilt package layers:

1. **In-Memory Boot Initramfs (`build/initramfs.cpio`)**:
   - Lightweight CPIO archive containing the PID 1 `/init` script, essential core utilities (`busybox`), dynamic linker (`ld-musl-x86_64.so.1`), and recovery shell fallback.
   - Mounts pseudo-filesystems (`/proc`, `/sys`, `/dev`, `/run`), detects block storage (`/dev/sda` / `/dev/vda`), mounts the ext4 rootfs to `/sysroot`, prepares nested devpts/sysfs/procfs, and transitions into the full Alpine rootfs environment.

2. **Full Rootfs Block Image (`build/ext4.img`)**:
   - Complete ext4 filesystem image populated with standard Alpine Linux minirootfs base, `openrc` init system, and prebuilt `developer-tools` layers (`bash`, `python3`, `btop`, `e2fsprogs`, `exfatprogs`, GCC/build-base, etc.).

---

## 2. Rootfs Packages & Repositories

The userland build script (`scripts/build-userland/build-userland-amd64.sh`) uses prebuilt package scripts in `scripts/prebuild-package/` and static APK (`apk.static`) to provision packages without requiring host root privileges.

### Configured Repositories
- Alpine Linux Main (`https://dl-cdn.alpinelinux.org/alpine/v3.20/main`)
- Alpine Linux Community (`https://dl-cdn.alpinelinux.org/alpine/v3.20/community`)

### Installed Package Layers
- `openrc`: OpenRC service manager and init scripts.
- `developer-tools`: `bash`, `python3`, `btop`, `e2fsprogs`, `exfatprogs`, `build-base`, `coreutils`, etc.

---

## 3. Building the Userland

### Prerequisites
The host system requires (Ubuntu / Ubuntu on WSL):
- `bash`, `curl`, `tar`, `file`, `cpio`, `mke2fs` (`e2fsprogs`)
- Internet connectivity to fetch Alpine minirootfs and package archives.

### Running the Builder
Run the userland assembly script from the repository root:

```bash
# Assembles the Alpine-based userland, staging directories, and images
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

This generates:
- `build/initramfs.cpio`: Boot CPIO image configured in `OSDK.toml` (`initramfs = "build/initramfs.cpio"`).
- `build/ext4.img`: ext4 disk image attached to the AHCI bus in QEMU.

---

## 5. Maintenance & Attribution

* All build scripts and configuration updates maintain strict human maintainer attribution (`git config user.name` / `user.email`).
* Kernel build checks (`cargo osdk check` and `cargo osdk build`) must always pass with exit code `0`.
