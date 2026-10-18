#!/bin/bash
# Remove generated PetraOS build artifacts so the next build starts clean.
#
# Usage: clean.sh [OPTIONS]
#
# Options:
#   --userland   Userland staging, package layers, initramfs and block images
#                (default when no option is given)
#   --iso        Bootable ISO and its staging tree
#   --cache      Downloaded caches (Alpine minirootfs, apk.static, apk index)
#   --kernel     Rust kernel build output (target/)
#   --all        Everything above
#   -h, --help   Show this help message
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR="${REPO_ROOT}/build"

CLEAN_USERLAND=0
CLEAN_ISO=0
CLEAN_CACHE=0
CLEAN_KERNEL=0

usage() {
    echo "Usage: $0 [--userland] [--iso] [--cache] [--kernel] [--all]"
    echo ""
    echo "  --userland   Userland staging, package layers, initramfs and block images (default)"
    echo "  --iso        Bootable ISO and its staging tree"
    echo "  --cache      Downloaded caches (Alpine minirootfs, apk.static, apk index)"
    echo "  --kernel     Rust kernel build output (target/)"
    echo "  --all        Everything above"
    echo "  -h, --help   Show this help message"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --userland)
            CLEAN_USERLAND=1
            ;;
        --iso)
            CLEAN_ISO=1
            ;;
        --cache)
            CLEAN_CACHE=1
            ;;
        --kernel|--target)
            CLEAN_KERNEL=1
            ;;
        --all)
            CLEAN_USERLAND=1
            CLEAN_ISO=1
            CLEAN_CACHE=1
            CLEAN_KERNEL=1
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

if [ $((CLEAN_USERLAND + CLEAN_ISO + CLEAN_CACHE + CLEAN_KERNEL)) -eq 0 ]; then
    CLEAN_USERLAND=1
fi

remove() {
    local path="$1"
    if [ -e "$path" ]; then
        echo "    removing ${path#"${REPO_ROOT}"/}"
        rm -rf -- "$path"
    fi
}

if [ "${CLEAN_USERLAND}" -eq 1 ]; then
    echo "==> Cleaning userland build artifacts"
    remove "${BUILD_DIR}/userland-amd64"
    remove "${BUILD_DIR}/prebuild-package"
    remove "${BUILD_DIR}/initramfs.cpio"
    remove "${BUILD_DIR}/initramfs_base"
    remove "${BUILD_DIR}/ext2.img"
    remove "${BUILD_DIR}/exfat.img"
    remove "${BUILD_DIR}/disk.img"
fi

if [ "${CLEAN_ISO}" -eq 1 ]; then
    echo "==> Cleaning ISO artifacts"
    remove "${BUILD_DIR}/iso_root"
    remove "${BUILD_DIR}/petraos-x86_64.iso"
fi

if [ "${CLEAN_CACHE}" -eq 1 ]; then
    echo "==> Cleaning download caches"
    remove "${BUILD_DIR}/cache"
fi

if [ "${CLEAN_KERNEL}" -eq 1 ]; then
    echo "==> Cleaning kernel build output"
    remove "${REPO_ROOT}/target"
fi

echo "Done."
