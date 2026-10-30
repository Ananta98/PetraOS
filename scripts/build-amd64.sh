#!/bin/bash
# Build the full PetraOS amd64 target: Rocky Linux userland + initramfs, the petra
# kernel, and a bootable hybrid BIOS/UEFI ISO.
#
# Usage: build-amd64.sh [--no-userland] [--no-iso]
#
#   --no-userland  keep the existing initramfs instead of rebuilding it
#   --no-iso       build the kernel and initramfs only
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"
BUILD_USERLAND=1
BUILD_ISO=1

for arg in "$@"; do
    case "$arg" in
        --no-userland) BUILD_USERLAND=0 ;;
        --no-iso) BUILD_ISO=0 ;;
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
    "${SCRIPT_DIR}/build-userland/build-userland-amd64.sh"
elif [ ! -f "${INITRAMFS}" ]; then
    echo "ERROR: --no-userland needs ${INITRAMFS}" >&2
    exit 1
fi

if [ -f "${SCRIPT_DIR}/build-support/vdso/build-vdso.sh" ]; then
    echo "==> Building the vDSO shared library"
    "${SCRIPT_DIR}/build-support/vdso/build-vdso.sh"
fi

echo "==> Building the petra kernel"
(cd "${REPO_ROOT}" && cargo osdk build)

if [ "${BUILD_ISO}" -eq 1 ]; then
    "${SCRIPT_DIR}/build-iso.sh"
fi
