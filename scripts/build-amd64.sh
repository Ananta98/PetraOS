#!/bin/bash
# Build the full PetraOS amd64 target: Rocky Linux userland + initramfs, the petra
# kernel, and a bootable hybrid BIOS/UEFI ISO.
#
# Usage: build-amd64.sh [OPTIONS]
#
# Options:
#   --fresh          Rebuild userland and rootfs staging from scratch
#   --purge-cache    With --fresh, also re-download the base Rocky Linux rootfs
#   --release        Build kernel in release profile (cargo osdk build --release)
#   --tools "TOOLS"  Additional tools to include in the initramfs
#   --userland-only  Build userland & initramfs only (skip kernel and ISO)
#   --kernel-only    Build kernel only (skip userland and ISO)
#   --no-userland    Skip userland build (reuse existing initramfs)
#   --no-kernel      Skip kernel compilation
#   --no-iso         Skip bootable ISO creation
#   -h, --help       Show this help message
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"
BUILD_USERLAND=1
BUILD_KERNEL=1
BUILD_ISO=1
KERNEL_PROFILE="dev"
USERLAND_ARGS=()

usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS]

Build the full PetraOS amd64 target (Rocky Linux userland, Petra kernel, and hybrid ISO).

Options:
  --fresh          Rebuild userland staging from scratch
  --purge-cache    Purge cache and re-download base Rocky Linux rootfs
  --release        Compile kernel in release profile
  --tools "TOOLS"  Space-separated list of additional tools for initramfs
  --userland-only  Build userland and initramfs only (no kernel, no ISO)
  --kernel-only    Build kernel only (no userland, no ISO)
  --no-userland    Skip userland build (re-use existing initramfs)
  --no-kernel      Skip kernel build
  --no-iso         Skip ISO generation
  -h, --help       Show this help message
EOF
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        --fresh)
            USERLAND_ARGS+=("--fresh")
            ;;
        --purge-cache)
            USERLAND_ARGS+=("--purge-cache")
            ;;
        --tools)
            USERLAND_ARGS+=("--tools" "$2")
            shift
            ;;
        --release)
            KERNEL_PROFILE="release"
            ;;
        --userland-only)
            BUILD_USERLAND=1
            BUILD_KERNEL=0
            BUILD_ISO=0
            ;;
        --kernel-only)
            BUILD_USERLAND=0
            BUILD_KERNEL=1
            BUILD_ISO=0
            ;;
        --no-userland)
            BUILD_USERLAND=0
            ;;
        --no-kernel)
            BUILD_KERNEL=0
            ;;
        --no-iso)
            BUILD_ISO=0
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "ERROR: unknown option: $1" >&2
            usage
            ;;
    esac
    shift
done

export PATH="${HOME}/.cargo/bin:${PATH}"

START_TIME=$(date +%s)

echo "============================================================"
echo " PetraOS amd64 Build Pipeline"
echo " Target: Rocky Linux 9 (x86_64) | Profile: ${KERNEL_PROFILE}"
echo "============================================================"

# Step 1: Userland & Initramfs
if [ "${BUILD_USERLAND}" -eq 1 ]; then
    echo ""
    echo "[1/3] Building Rocky Linux userland and initramfs..."
    "${SCRIPT_DIR}/build-userland/build-userland-amd64.sh" "${USERLAND_ARGS[@]}"
elif [ ! -f "${INITRAMFS}" ]; then
    echo "ERROR: --no-userland specified, but ${INITRAMFS} not found." >&2
    echo "       Run without --no-userland first to build the initramfs." >&2
    exit 1
else
    echo ""
    echo "[1/3] Reusing existing userland and initramfs (${INITRAMFS})"
fi

# Step 2: Kernel Build
if [ "${BUILD_KERNEL}" -eq 1 ]; then
    echo ""
    echo "[2/3] Building PetraOS kernel (cargo osdk build)..."
    command -v cargo >/dev/null 2>&1 || {
        echo "ERROR: cargo not found. Ensure Rust toolchain is installed in ~/.cargo/bin" >&2
        exit 1
    }

    CARGO_FLAGS=()
    if [ "${KERNEL_PROFILE}" = "release" ]; then
        CARGO_FLAGS+=("--release")
    fi

    (cd "${REPO_ROOT}" && cargo osdk build "${CARGO_FLAGS[@]}")
else
    echo ""
    echo "[2/3] Skipping kernel build (--no-kernel / --userland-only)"
fi

# Step 3: Bootable ISO
if [ "${BUILD_ISO}" -eq 1 ]; then
    echo ""
    echo "[3/3] Creating hybrid bootable ISO..."
    "${SCRIPT_DIR}/build-iso.sh"
else
    echo ""
    echo "[3/3] Skipping ISO creation (--no-iso)"
fi

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

echo ""
echo "============================================================"
echo " Build completed in ${ELAPSED}s"
echo "============================================================"
