#!/bin/bash
# Pack a staged userland into a newc CPIO initramfs and build the ext2/exFAT
# block-device images that the kernel mounts (see OSDK.toml's qemu args).
#
# The filesystem images are formatted with the e2fsprogs/exfatprogs tools that
# the userland already staged, run through the staged musl loader. Building the
# images therefore does not depend on the host having e2fsprogs or exfatprogs.
#
# Usage: build-initramfs.sh [--staging DIR] [--output CPIO]
#                           [--ext2 FILE] [--exfat FILE]
#                           [--image-size SIZE] [--no-images]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

STAGING="${REPO_ROOT}/build/userland-amd64/staging"
OUTPUT="${REPO_ROOT}/build/initramfs.cpio"
EXT2_IMAGE="${REPO_ROOT}/build/ext2.img"
EXFAT_IMAGE="${REPO_ROOT}/build/exfat.img"
IMAGE_SIZE="64M"
BUILD_IMAGES=1

usage() {
    echo "Usage: $0 [--staging DIR] [--output CPIO] [--ext2 FILE] [--exfat FILE]"
    echo "          [--image-size SIZE] [--no-images]"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --staging) STAGING="$2"; shift 2 ;;
        --output) OUTPUT="$2"; shift 2 ;;
        --ext2) EXT2_IMAGE="$2"; shift 2 ;;
        --exfat) EXFAT_IMAGE="$2"; shift 2 ;;
        --image-size) IMAGE_SIZE="$2"; shift 2 ;;
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

MUSL_LOADER="${STAGING}/lib/ld-musl-x86_64.so.1"

# Run a tool that lives inside the staging root using the staging root's own
# musl loader, which works regardless of the host C library.
run_staged() {
    local tool="$1"
    shift
    "${MUSL_LOADER}" --library-path "${STAGING}/lib:${STAGING}/usr/lib" \
        "${STAGING}/${tool}" "$@"
}

make_image() {
    local image="$1" size="$2"
    mkdir -p "$(dirname "${image}")"
    rm -f "${image}"
    truncate -s "${size}" "${image}"
}

build_ext2_image() {
    make_image "${EXT2_IMAGE}" "${IMAGE_SIZE}"
    if [ -x "${MUSL_LOADER}" ] && [ -x "${STAGING}/sbin/mkfs.ext2" ]; then
        run_staged sbin/mkfs.ext2 -q -F "${EXT2_IMAGE}"
        echo "    ext2:  ${EXT2_IMAGE} (${IMAGE_SIZE})"
    elif command -v mkfs.ext2 >/dev/null 2>&1; then
        mkfs.ext2 -q -F "${EXT2_IMAGE}"
        echo "    ext2:  ${EXT2_IMAGE} (${IMAGE_SIZE}, host mkfs.ext2)"
    else
        echo "    ext2:  ${EXT2_IMAGE} left unformatted (no mkfs.ext2 available)" >&2
    fi
}

build_exfat_image() {
    make_image "${EXFAT_IMAGE}" "${IMAGE_SIZE}"
    if [ -x "${MUSL_LOADER}" ] && [ -x "${STAGING}/usr/sbin/mkfs.exfat" ]; then
        run_staged usr/sbin/mkfs.exfat "${EXFAT_IMAGE}" >/dev/null
        echo "    exfat: ${EXFAT_IMAGE} (${IMAGE_SIZE})"
    elif command -v mkfs.exfat >/dev/null 2>&1; then
        mkfs.exfat "${EXFAT_IMAGE}" >/dev/null
        echo "    exfat: ${EXFAT_IMAGE} (${IMAGE_SIZE}, host mkfs.exfat)"
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
echo "    ${OUTPUT} ($(du -h "${OUTPUT}" | cut -f1), $(find "${STAGING}" -mindepth 1 | wc -l | tr -d ' ') entries)"

if [ "${BUILD_IMAGES}" -eq 1 ]; then
    echo "==> Building block-device images"
    build_ext2_image
    build_exfat_image
fi
