#!/bin/bash
# Build the full PetraOS amd64 target: postmarketOS userland + initramfs,
# the petra kernel, and a bootable hybrid BIOS/UEFI ISO.
#
# Usage: build-amd64.sh [OPTIONS]
#
# Options:
#   --fresh        rebuild userland and rootfs staging from scratch
#   --purge-cache  with --fresh, also drop the download cache and re-fetch Alpine
#   --no-userland  keep the existing initramfs instead of rebuilding it
#   --no-iso       build the kernel and initramfs only
#   --release      build kernel in release profile
#   -h, --help     show this help message
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"
BUILD_USERLAND=1
BUILD_ISO=1
KERNEL_PROFILE="dev"
USERLAND_ARGS=()

for arg in "$@"; do
    case "$arg" in
        --fresh|--purge-cache)
            USERLAND_ARGS+=("$arg")
            ;;
        --no-userland)
            BUILD_USERLAND=0
            ;;
        --no-iso)
            BUILD_ISO=0
            ;;
        --release)
            KERNEL_PROFILE="release"
            ;;
        -h|--help)
            sed -n '2,/^set -/s/^# \{0,1\}//p' "$0"
            exit 0
            ;;
        *)
            echo "ERROR: unknown option: $arg" >&2
            exit 1
            ;;
    esac
done

export PATH="${HOME}/.cargo/bin:${PATH}"

if [ "${BUILD_USERLAND}" -eq 1 ]; then
    echo "==> Building userland and initramfs..."
    "${SCRIPT_DIR}/build-userland/build-userland-amd64.sh" "${USERLAND_ARGS[@]}"
elif [ ! -f "${INITRAMFS}" ]; then
    echo "ERROR: --no-userland needs ${INITRAMFS}" >&2
    exit 1
fi

echo "==> Building the petra kernel"
CARGO_FLAGS=()
if [ "${KERNEL_PROFILE}" = "release" ]; then
    CARGO_FLAGS+=("--release")
fi
(cd "${REPO_ROOT}" && cargo osdk build "${CARGO_FLAGS[@]}")

if [ "${BUILD_ISO}" -eq 1 ]; then
    echo "==> Building bootable ISO..."
    "${SCRIPT_DIR}/build-iso.sh"
fi
