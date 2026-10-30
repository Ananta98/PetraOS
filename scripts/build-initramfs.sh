#!/bin/bash
# Pack a staged userland into a newc CPIO initramfs and build the ext4/exFAT
# block-device images that the kernel mounts (see OSDK.toml's qemu args).
#
# The filesystem images are formatted with the e2fsprogs/exfatprogs tools that
# the userland already staged, or available on the host system.
#
# Usage: build-initramfs.sh [--staging DIR] [--output CPIO]
#                           [--ext4 FILE] [--exfat FILE]
#                           [--image-size SIZE] [--no-images]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

STAGING="${REPO_ROOT}/build/userland-amd64/staging"
ROOTFS_STAGING="${REPO_ROOT}/build/userland-amd64/rootfs_staging"
OUTPUT="${REPO_ROOT}/build/initramfs.cpio"
EXT4_IMAGE="${REPO_ROOT}/build/ext4.img"
EXFAT_IMAGE="${REPO_ROOT}/build/exfat.img"
IMAGE_SIZE="1G"
BUILD_IMAGES=1
COMPRESS=0

usage() {
    echo "Usage: $0 [--staging DIR] [--rootfs-staging DIR] [--output CPIO]"
    echo "          [--ext4 FILE] [--exfat FILE] [--image-size SIZE]"
    echo "          [--compress] [--no-images]"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --staging) STAGING="$2"; shift 2 ;;
        --rootfs-staging) ROOTFS_STAGING="$2"; shift 2 ;;
        --output) OUTPUT="$2"; shift 2 ;;
        --ext4) EXT4_IMAGE="$2"; shift 2 ;;
        --exfat) EXFAT_IMAGE="$2"; shift 2 ;;
        --image-size) IMAGE_SIZE="$2"; shift 2 ;;
        --compress) COMPRESS=1; shift ;;
        --no-images) BUILD_IMAGES=0; shift ;;
        -h|--help) usage ;;
        *)
            echo "ERROR: unknown option: $1" >&2
            usage
            ;;
    esac
done

for tool in cpio find sort; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "ERROR: required command not found: $tool" >&2
        exit 1
    }
done

if [ ! -d "${STAGING}" ]; then
    echo "ERROR: staging directory not found: ${STAGING}" >&2
    echo "       run scripts/build-userland/build-userland-amd64.sh first" >&2
    exit 1
fi

GLIBC_LOADER="${STAGING}/lib64/ld-linux-x86-64.so.2"
[ -e "${GLIBC_LOADER}" ] || GLIBC_LOADER="${STAGING}/usr/lib64/ld-linux-x86-64.so.2"

# Run a tool that lives inside the staging root using the staging root's own
# dynamic loader, which works regardless of the host C library.
run_staged() {
    local tool="$1"
    shift
    if [ -x "${GLIBC_LOADER}" ]; then
        "${GLIBC_LOADER}" --library-path "${STAGING}/lib64:${STAGING}/usr/lib64:${STAGING}/lib:${STAGING}/usr/lib" \
            "${STAGING}/${tool}" "$@"
    fi
}

make_image() {
    local image="$1" size="$2"
    mkdir -p "$(dirname "${image}")"
    rm -f "${image}"
    truncate -s "${size}" "${image}"
}

build_ext4_image() {
    make_image "${EXT4_IMAGE}" "${IMAGE_SIZE}"
    local populate_dir="${ROOTFS_STAGING}"
    if [ ! -d "${populate_dir}" ]; then
        populate_dir="${STAGING}"
    fi

    # Format ext4 with directory pre-population if rootfs directory exists
    if command -v mke2fs >/dev/null 2>&1; then
        if [ -d "${populate_dir}" ]; then
            mke2fs -t ext4 -F -O ^metadata_csum,^64bit -d "${populate_dir}" "${EXT4_IMAGE}" >/dev/null
            echo "    ext4:  ${EXT4_IMAGE} (${IMAGE_SIZE}, populated from ${populate_dir})"
        else
            mke2fs -t ext4 -F -O ^metadata_csum,^64bit "${EXT4_IMAGE}" >/dev/null
            echo "    ext4:  ${EXT4_IMAGE} (${IMAGE_SIZE}, empty)"
        fi
    elif [ -x "${GLIBC_LOADER}" ] && [ -x "${STAGING}/usr/sbin/mkfs.ext4" ]; then
        run_staged usr/sbin/mkfs.ext4 -q -F "${EXT4_IMAGE}"
        echo "    ext4:  ${EXT4_IMAGE} (${IMAGE_SIZE}, staged mkfs.ext4)"
    elif [ -x "${GLIBC_LOADER}" ] && [ -x "${STAGING}/sbin/mkfs.ext4" ]; then
        run_staged sbin/mkfs.ext4 -q -F "${EXT4_IMAGE}"
        echo "    ext4:  ${EXT4_IMAGE} (${IMAGE_SIZE}, staged mkfs.ext4)"
    elif command -v mkfs.ext4 >/dev/null 2>&1; then
        mkfs.ext4 -q -F "${EXT4_IMAGE}"
        echo "    ext4:  ${EXT4_IMAGE} (${IMAGE_SIZE}, host mkfs.ext4)"
    else
        echo "    ext4:  ${EXT4_IMAGE} left unformatted (no mkfs.ext4 available)" >&2
    fi
}

build_exfat_image() {
    make_image "${EXFAT_IMAGE}" "64M"
    if [ -x "${GLIBC_LOADER}" ] && [ -x "${STAGING}/usr/sbin/mkfs.exfat" ]; then
        run_staged usr/sbin/mkfs.exfat "${EXFAT_IMAGE}" >/dev/null
        echo "    exfat: ${EXFAT_IMAGE} (64M)"
    elif [ -x "${GLIBC_LOADER}" ] && [ -x "${STAGING}/sbin/mkfs.exfat" ]; then
        run_staged sbin/mkfs.exfat "${EXFAT_IMAGE}" >/dev/null
        echo "    exfat: ${EXFAT_IMAGE} (64M)"
    elif command -v mkfs.exfat >/dev/null 2>&1; then
        mkfs.exfat "${EXFAT_IMAGE}" >/dev/null
        echo "    exfat: ${EXFAT_IMAGE} (64M, host mkfs.exfat)"
    else
        echo "    exfat: ${EXFAT_IMAGE} left unformatted (no mkfs.exfat available)" >&2
    fi
}

echo "==> Packing initramfs: ${OUTPUT}"
mkdir -p "$(dirname "${OUTPUT}")"
(
    cd "${STAGING}"
    find . -mindepth 1 | sort | cpio -o -H newc --quiet > "${OUTPUT}"
)

if [ "${COMPRESS}" -eq 1 ]; then
    if command -v gzip >/dev/null 2>&1; then
        echo "==> Compressing initramfs with gzip: ${OUTPUT}.gz"
        gzip -9 -c "${OUTPUT}" > "${OUTPUT}.gz"
        echo "    ${OUTPUT}.gz ($(du -h "${OUTPUT}.gz" | cut -f1))"
    fi
fi

echo "    ${OUTPUT} ($(du -h "${OUTPUT}" | cut -f1), $(find "${STAGING}" -mindepth 1 | wc -l | tr -d ' ') entries)"

if [ "${BUILD_IMAGES}" -eq 1 ]; then
    echo "==> Building block-device images"
    build_ext4_image
    build_exfat_image
fi
