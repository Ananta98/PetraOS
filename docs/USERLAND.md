# PetraOS Userland Build & Packaging Guide

This document describes the userspace architecture, package provisioning workflow, EPEL repository integration, and rootfs image generation for **PetraOS**.

---

## 1. Overview

PetraOS uses a hybrid two-tier userspace model based on **Rocky Linux 9 (x86_64)**:

1. **In-Memory Boot Initramfs (`build/initramfs.cpio`)**:
   - Lightweight CPIO archive containing the PID 1 `/init` script, essential core utilities (`bash`, `coreutils`, `mount`, `chroot`), OpenRC binaries (`openrc`, `openrc-init`, `rc-service`), and dynamically resolved ELF shared libraries (`libc`, `librc`, etc.).
   - Mounts pseudo-filesystems (`/proc`, `/sys`, `/dev`, `/run`), detects block storage (`/dev/sda` / `/dev/vda`), mounts ext4 rootfs to `/sysroot`, and transitions to OpenRC or an interactive shell.

2. **Full Rootfs Block Image (`build/ext4.img`)**:
   - Complete 2 GB ext4 filesystem image populated with standard Rocky Linux 9 distribution packages, OpenRC init system, compiler toolchain (`gcc`, `g++`, `make`, `gdb`), scripting runtimes (`python3`, `lua`), system utilities, and EPEL packages such as `fastfetch`.

---

## 2. EPEL Repository & Fastfetch

### Why EPEL?
Rocky Linux base repositories contain enterprise-grade server packages. Community system utilities, modern system-information tools (like `fastfetch`), and development tools reside in the **Extra Packages for Enterprise Linux (EPEL)** repository.

### Configuration in `build-userland-amd64.sh`
The builder automatically provisions `epel-release` before attempting to install EPEL-specific packages:

```bash
# 1. Ensure EPEL repository metadata is enabled
unshare -r chroot "${ROOTFS_STAGING}" /usr/bin/dnf install -y \
    --setopt=tsflags=nodocs \
    --setopt=install_weak_deps=False \
    --nogpgcheck \
    epel-release

# 2. Install packages including fastfetch from EPEL and build tools from CRB
DEFAULT_ROOTFS_PACKAGES=(
    gcc gcc-c++ gdb make binutils
    file which findutils diffutils
    procps-ng iproute less nano
    tar gzip xz bzip2
    libcap-devel meson ninja-build
    python3 lua
    epel-release fastfetch
)
```

---

## 3. Building the Userland

### Prerequisites
The host system requires:
- `bash`, `tar`, `xz`, `cpio`, `mke2fs` (`e2fsprogs`), `unshare` (`util-linux`)
- Internet connectivity to fetch the Rocky Linux base container and DNF package metadata

### Running the Builder
Run the userland assembly script from the repository root:

```bash
# Full build: downloads Rocky Linux 9 base, installs DNF packages (including epel-release and fastfetch),
# copies ELF dependencies, and creates build/initramfs.cpio and build/ext4.img
bash scripts/build-userland/build-userland-amd64.sh
```

### Useful Options:
- `--fresh`: Cleans staging directories and downloads fresh upstream images.
- `--no-cache`: Re-downloads package metadata without using local DNF cache.
- `--skip-rootfs`: Skips package provisioning if the rootfs staging directory is already prepared.

---

## 4. Packing Initramfs & Filesystem Images

If you modify staging directories directly, you can repack without re-downloading packages:

```bash
bash scripts/build-initramfs.sh
```

This generates:
- `build/initramfs.cpio`: Boot CPIO image passed via `OSDK.toml` (`initramfs = "build/initramfs.cpio"`).
- `build/ext4.img`: 2 GB ext4 disk image attached to AHCI bus in QEMU.

---

## 5. Running Fastfetch in PetraOS

Once PetraOS boots into the interactive console:

```bash
# Execute fastfetch to inspect system information, kernel version, and hardware specs
fastfetch
```

Output highlights:
- **OS**: PetraOS (Rocky Linux 9)
- **Kernel**: PetraOS (x86_64)
- **Shell**: GNU bash
- **Uptime & Memory**: Reported from PetraOS `/proc` and sysfs subsystems.

You can also install additional EPEL packages live at runtime whenever networking is enabled:
```bash
dnf install -y <package-name>
```

---

## 6. Maintenance & Attribution

* All build scripts and configuration updates maintain strict human maintainer attribution (`git config user.name` / `user.email`).
* Kernel build checks (`cargo osdk check` and `cargo osdk build`) must always pass with exit code `0` when updating userland configuration or sysfs/VFS hooks.

