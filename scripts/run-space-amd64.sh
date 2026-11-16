#!/bin/bash
# Build (if needed) and boot PetraOS directly into the PetraSpace custom GUI in QEMU.
#
# Usage: ./scripts/run-space-amd64.sh [--no-build] [--release] [cargo osdk run args...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

BUILD_USERLAND=1
PASSTHROUGH=()

for arg in "$@"; do
    case "$arg" in
        --no-build)
            BUILD_USERLAND=0
            ;;
        --help|-h)
            sed -n '2,/^set -/s/^# \{0,1\}//p' "$0"
            exit 0
            ;;
        *)
            PASSTHROUGH+=("$arg")
            ;;
    esac
done

export PATH="${HOME}/.cargo/bin:${PATH}"

if [ "${BUILD_USERLAND}" -eq 1 ]; then
    if [ ! -x "${REPO_ROOT}/build/userland-amd64/rootfs_staging/usr/bin/space-compositor" ] || \
       [ ! -f "${REPO_ROOT}/build/ext4.img" ] || \
       [ ! -f "${REPO_ROOT}/build/initramfs.cpio" ]; then
        echo "==> Assembling userland + PetraSpace GUI rootfs image..."
        "${SCRIPT_DIR}/build-userland/build-userland-amd64.sh"
    fi
fi

echo "==> Booting PetraOS directly into PetraSpace GUI..."
cd "${REPO_ROOT}"
exec cargo osdk run --kcmd-args="PETRA_BOOT_GUI=1" "${PASSTHROUGH[@]}"
