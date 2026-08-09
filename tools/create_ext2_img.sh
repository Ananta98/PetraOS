#!/usr/bin/env bash
set -euo pipefail

IMG_NAME="ext2.img"
SIZE_MB="4"

if ! command -v mkfs.ext2 &>/dev/null; then
    echo "[ERROR] 'mkfs.ext2' not found." >&2
    exit 1
fi

dd if=/dev/zero of="${IMG_NAME}" bs=1M count="${SIZE_MB}" status=none
mkfs.ext2 -F -r 0 -b 1024 -I 128 -O none "${IMG_NAME}" >/dev/null
echo "[INFO] Formatted ${IMG_NAME} (Ext2 Rev 0, 1024B block size)"