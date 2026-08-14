#!/usr/bin/env bash
# ==============================================================================
# PetraOS Initramfs Creation Tool
#
# Packages a directory tree into a standard SVR4 portable format (newc) CPIO archive.
# ==============================================================================

set -euo pipefail

ROOT_DIR="${1:-initramfs_root}"
OUTPUT_CPIO="${2:-initramfs.cpio}"

# Ensure source root directory exists
if [ ! -d "${ROOT_DIR}" ]; then
    echo "[INFO] Creating directory tree '${ROOT_DIR}'..."
    mkdir -p "${ROOT_DIR}/sbin" "${ROOT_DIR}/bin" "${ROOT_DIR}/etc"
fi

echo "[INFO] Packaging '${ROOT_DIR}' into '${OUTPUT_CPIO}'..."

if ! command -v cpio &>/dev/null; then
    echo "[ERROR] 'cpio' command not found. Please install cpio (e.g. sudo apt install cpio)." >&2
    exit 1
fi

(
    cd "${ROOT_DIR}"
    find . -mindepth 1 | cpio -o -H newc -R 0:0 > "../${OUTPUT_CPIO}"
)

echo "✔ [INFO] Generated ${OUTPUT_CPIO} ($(wc -c < "${OUTPUT_CPIO}") bytes)"
