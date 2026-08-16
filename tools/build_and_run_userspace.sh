#!/usr/bin/env bash
# ==============================================================================
# PetraOS Userspace Build & Run Script
#
# Automated pipeline:
# 1. Initialize xbstrap workspace
# 2. Download / Fetch source packages
# 3. Build toolchain and userspace packages into sysroot
# 4. Sync sysroot binaries/libraries into initramfs
# 5. Build bootable ISO and launch in QEMU
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${ROOT_DIR}"

MODE="${1:-default}"
QEMU_EXTRA_FLAGS="${QEMUFLAGS:--m 2G -serial stdio}"

echo "============================================================"
echo "          PetraOS Userspace Build & Launch Pipeline         "
echo "============================================================"

# Step 1: Initialize xbstrap
echo "[1/5] Initializing xbstrap workspace..."
make xbstrap-init

# Step 2: Download / Fetch packages
if [ "${MODE}" = "--all" ] || [ "${MODE}" = "all" ]; then
    echo "[2/5] Downloading all sources via xbstrap-fetch..."
    make xbstrap-fetch
    echo "[3/5] Building all userspace packages..."
    make userspace-all
else
    echo "[2/5] Fetching core packages (mlibc, bash)..."
    (cd build-xbstrap && xbstrap fetch mlibc bash)
    echo "[3/5] Building core userspace (mlibc-headers, mlibc, bash)..."
    make userspace
fi

# Step 4: Package initramfs
echo "[4/5] Packaging initramfs cpio archive..."
make initramfs

# Step 5: Build ISO & launch QEMU
echo "[5/5] Launching PetraOS in QEMU..."
make run QEMUFLAGS="${QEMU_EXTRA_FLAGS}"
