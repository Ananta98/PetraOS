#!/bin/bash
# ==============================================================================
# PetraOS Silica Kernel - QEMU x86_64 Runner
# ==============================================================================
# Boots the PetraOS Silica kernel with its PetraOS userspace in QEMU.
# Supports both direct kernel/initrd loading and bootable ISO execution,
# with configurable QMP socket for screen capture and headless automation.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

KERNEL_BIN="${REPO_ROOT}/target/osdk/silica-osdk-bin"
INITRAMFS_CPIO="${REPO_ROOT}/build/initramfs.cpio"
ISO_IMAGE="${REPO_ROOT}/build/petraos-x86_64.iso"
EXT2_IMAGE="${REPO_ROOT}/build/ext2.img"
EXFAT_IMAGE="${REPO_ROOT}/build/exfat.img"
QMP_SOCKET="/tmp/qmp-petra.sock"

BOOT_MODE="direct" # "direct" or "iso"
GRAPHIC=false
MEM="4G"
SMP="4"
ENABLE_KVM="auto"
EXTRA_QEMU_ARGS=()
KCMD_LINE="SHELL=/bin/sh LOGNAME=root HOME=/root USER=root PATH=/bin:/sbin:/usr/bin:/usr/sbin init=/init -- sh -l"

usage() {
    echo "Usage: $0 [OPTIONS] [-- EXTRA_QEMU_ARGS...]"
    echo ""
    echo "Options:"
    echo "  -g, --graphic           Launch with graphical display window (default: terminal/nographic)"
    echo "  -n, --nographic         Run headless with serial in terminal (default)"
    echo "  --iso                   Boot from ISO image (build/petraos-x86_64.iso)"
    echo "  --direct                Direct kernel boot with initramfs (default)"
    echo "  --mem <SIZE>            RAM allocation (default: 4G)"
    echo "  --smp <CORES>           Virtual CPU count (default: 4)"
    echo "  --kvm                   Force enable KVM acceleration"
    echo "  --no-kvm                Disable KVM acceleration"
    echo "  --qmp <SOCKET>          Path to QMP unix socket (default: /tmp/qmp-petra.sock)"
    echo "  --cmdline <ARGS>        Kernel commandline string"
    echo "  -h, --help              Show this help message"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -g|--graphic)
            GRAPHIC=true
            shift
            ;;
        -n|--nographic)
            GRAPHIC=false
            shift
            ;;
        --iso)
            BOOT_MODE="iso"
            shift
            ;;
        --direct)
            BOOT_MODE="direct"
            shift
            ;;
        --mem)
            MEM="$2"
            shift 2
            ;;
        --smp)
            SMP="$2"
            shift 2
            ;;
        --kvm)
            ENABLE_KVM="true"
            shift
            ;;
        --no-kvm)
            ENABLE_KVM="false"
            shift
            ;;
        --qmp)
            QMP_SOCKET="$2"
            shift 2
            ;;
        --cmdline)
            KCMD_LINE="$2"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        --)
            shift
            EXTRA_QEMU_ARGS=("$@")
            break
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

# Ensure build artifacts exist
if [ "${BOOT_MODE}" = "direct" ]; then
    if [ ! -f "${INITRAMFS_CPIO}" ]; then
        echo "[runner] Building initramfs..."
        "${REPO_ROOT}/scripts/build-userland/build-userland-amd64.sh"
    fi
    if [ ! -f "${KERNEL_BIN}" ]; then
        echo "[runner] Building kernel..."
        (cd "${REPO_ROOT}" && cargo osdk build)
    fi
else
    if [ ! -f "${ISO_IMAGE}" ]; then
        echo "[runner] Building bootable ISO..."
        "${REPO_ROOT}/scripts/build-iso.sh"
    fi
fi

# Detect KVM availability
KVM_FLAGS=()
if [ "${ENABLE_KVM}" = "true" ] || { [ "${ENABLE_KVM}" = "auto" ] && [ -w /dev/kvm ]; }; then
    KVM_FLAGS+=("-enable-kvm" "-cpu" "host")
else
    KVM_FLAGS+=("-cpu" "Icelake-Server,+x2apic")
fi

# QMP monitor setup
QMP_FLAGS=()
if [ -n "${QMP_SOCKET}" ]; then
    rm -f "${QMP_SOCKET}"
    QMP_FLAGS+=("-qmp" "unix:${QMP_SOCKET},server,nowait")
fi

# Display & serial setup
DISPLAY_FLAGS=()
if [ "${GRAPHIC}" = true ]; then
    DISPLAY_FLAGS+=("-vga" "std" "-serial" "stdio")
else
    DISPLAY_FLAGS+=("-display" "none" "-serial" "stdio")
fi

# OVMF UEFI Firmware support
OVMF_FLAGS=()
if [ -f "/usr/share/OVMF/OVMF_CODE_4M.fd" ] && [ -f "/usr/share/OVMF/OVMF_VARS_4M.fd" ]; then
    OVMF_FLAGS+=(
        -drive if=pflash,format=raw,unit=0,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd
        -drive if=pflash,format=raw,unit=1,file=/usr/share/OVMF/OVMF_VARS_4M.fd
    )
fi

# Attach the ext2/exFAT test images as AHCI drives when they have been built.
DISK_FLAGS=()
if [ -f "${EXT2_IMAGE}" ] || [ -f "${EXFAT_IMAGE}" ]; then
    DISK_FLAGS+=(-device ich9-ahci,id=ahci)
    if [ -f "${EXT2_IMAGE}" ]; then
        DISK_FLAGS+=(
            -drive id=ext2,file="${EXT2_IMAGE}",format=raw,if=none
            -device ide-hd,drive=ext2,bus=ahci.0
        )
    fi
    if [ -f "${EXFAT_IMAGE}" ]; then
        DISK_FLAGS+=(
            -drive id=exfat,file="${EXFAT_IMAGE}",format=raw,if=none
            -device ide-hd,drive=exfat,bus=ahci.1
        )
    fi
fi

echo "============================================================"
echo " Starting PetraOS Silica in QEMU"
echo " Mode:       ${BOOT_MODE}"
echo " Display:    $( [ "${GRAPHIC}" = true ] && echo 'Graphical' || echo 'Serial Terminal' )"
echo " Memory:     ${MEM}"
echo " Cores:      ${SMP}"
echo " QMP Socket: ${QMP_SOCKET}"
echo "============================================================"

# Construct QEMU command
QEMU_CMD=(
    qemu-system-x86_64
    -machine q35,kernel-irqchip=split
    "${KVM_FLAGS[@]}"
    -m "${MEM}"
    -smp "${SMP}"
    --no-reboot
    -device isa-debug-exit,iobase=0xf4,iosize=0x04
    "${DISPLAY_FLAGS[@]}"
    "${QMP_FLAGS[@]}"
    "${DISK_FLAGS[@]}"
    "${OVMF_FLAGS[@]}"
)

if [ "${BOOT_MODE}" = "direct" ]; then
    QEMU_CMD+=(
        -kernel "${KERNEL_BIN}"
        -initrd "${INITRAMFS_CPIO}"
        -append "${KCMD_LINE}"
    )
else
    QEMU_CMD+=(
        -cdrom "${ISO_IMAGE}"
        -boot d
    )
fi

if [ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]; then
    QEMU_CMD+=("${EXTRA_QEMU_ARGS[@]}")
fi

exec "${QEMU_CMD[@]}"
