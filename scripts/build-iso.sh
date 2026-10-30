#!/bin/bash
# ==============================================================================
# PetraOS Silica Kernel - Bootable x86_64 ISO Generator
# ==============================================================================
# This script builds a bootable x86_64 hybrid ISO image (BIOS + UEFI)
# containing the PetraOS Silica kernel, PetraOS initramfs, and GRUB.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

ISO_OUTPUT="${REPO_ROOT}/build/petraos-x86_64.iso"
ISO_ROOT="${REPO_ROOT}/build/iso_root"
KERNEL_BIN="${REPO_ROOT}/target/osdk/petra-osdk-bin"
INITRAMFS_CPIO="${REPO_ROOT}/build/initramfs.cpio"

USE_OSDK=false
VERBOSE=false

usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -o, --output <FILE>     Output ISO path (default: build/petraos-x86_64.iso)"
    echo "  -k, --kernel <FILE>     Kernel binary path (default: target/osdk/petra-osdk-bin)"
    echo "  -i, --initramfs <FILE>  Initramfs CPIO path (default: build/initramfs.cpio)"
    echo "  --osdk                  Use 'cargo osdk build --boot-method grub-rescue-iso' method"
    echo "  -v, --verbose           Enable verbose output"
    echo "  -h, --help              Show this help message"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -o|--output)
            ISO_OUTPUT="$2"
            shift 2
            ;;
        -k|--kernel)
            KERNEL_BIN="$2"
            shift 2
            ;;
        -i|--initramfs)
            INITRAMFS_CPIO="$2"
            shift 2
            ;;
        --osdk)
            USE_OSDK=true
            shift
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

echo "=== [1/4] Checking prerequisites ==="
mkdir -p "$(dirname "${ISO_OUTPUT}")"

# Ensure initramfs exists
if [ ! -f "${INITRAMFS_CPIO}" ]; then
    echo "Initramfs not found at ${INITRAMFS_CPIO}. Building the userland..."
    "${REPO_ROOT}/scripts/build-userland/build-userland-amd64.sh"
fi

# Ensure kernel is built
if [ ! -f "${KERNEL_BIN}" ] || [ "${USE_OSDK}" = true ]; then
    echo "Building kernel with cargo osdk..."
    (cd "${REPO_ROOT}" && cargo osdk build)
fi

if [ "${USE_OSDK}" = true ]; then
    echo "=== [2/4] Building ISO via cargo osdk (grub-rescue-iso) ==="
    (cd "${REPO_ROOT}" && cargo osdk build --boot-method grub-rescue-iso)
    if [ -f "${REPO_ROOT}/target/osdk/petra-osdk-bin.iso" ]; then
        cp "${REPO_ROOT}/target/osdk/petra-osdk-bin.iso" "${ISO_OUTPUT}"
        echo "ISO copied to ${ISO_OUTPUT}"
        exit 0
    fi
fi

echo "=== [2/4] Setting up GRUB ISO staging directory ==="
rm -rf "${ISO_ROOT}"
mkdir -p "${ISO_ROOT}/boot/grub"

cp "${KERNEL_BIN}" "${ISO_ROOT}/boot/vmlinuz-petraos"
cp "${INITRAMFS_CPIO}" "${ISO_ROOT}/boot/initramfs.cpio"

cat << 'EOF' > "${ISO_ROOT}/boot/grub/grub.cfg"
set timeout=3
set default=0

# Console configuration
insmod all_video
terminal_input console
terminal_output console

menuentry "PetraOS Silica Kernel (PetraOS Userspace)" {
    echo "Loading PetraOS Silica Kernel..."
    linux /boot/vmlinuz-petraos SHELL=/bin/sh LOGNAME=root HOME=/root USER=root PATH=/bin:/sbin:/usr/bin:/usr/sbin init=/init -- sh -l
    echo "Loading PetraOS Initramfs..."
    initrd /boot/initramfs.cpio
    echo "Booting PetraOS..."
    boot
}

menuentry "PetraOS Silica Kernel (Single-User /bin/sh direct)" {
    linux /boot/vmlinuz-petraos SHELL=/bin/sh LOGNAME=root HOME=/root USER=root PATH=/bin:/sbin init=/bin/sh -- sh -l
    initrd /boot/initramfs.cpio
    boot
}
EOF

echo "=== [3/4] Building bootable ISO with grub-mkrescue ==="
if command -v grub-mkrescue >/dev/null 2>&1; then
    grub-mkrescue -o "${ISO_OUTPUT}" "${ISO_ROOT}" 2>/dev/null
else
    echo "Error: grub-mkrescue not found. Please install grub-pc-bin and xorriso."
    exit 1
fi

echo "=== [4/4] Verifying generated ISO ==="
if [ -f "${ISO_OUTPUT}" ]; then
    ISO_SIZE=$(ls -lh "${ISO_OUTPUT}" | awk '{print $5}')
    echo ""
    echo "============================================================"
    echo " Bootable x86_64 ISO created successfully!"
    echo " Path: ${ISO_OUTPUT}"
    echo " Size: ${ISO_SIZE}"
    echo " Boot: BIOS + UEFI Hybrid"
    echo "============================================================"
else
    echo "Error: Failed to create ISO at ${ISO_OUTPUT}"
    exit 1
fi
