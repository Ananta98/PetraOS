#!/usr/bin/env bash
# ==============================================================================
# PetraOS Initramfs Creation Tool
#
# Packages a directory tree into a standard SVR4 portable format (newc) CPIO archive.
# ==============================================================================

set -euo pipefail

ROOT_DIR="${1:-build/initramfs_root}"
OUTPUT_CPIO="${2:-build/initramfs.cpio}"

# Ensure source root directory exists
if [ ! -d "${ROOT_DIR}" ]; then
    echo "[INFO] Creating directory tree '${ROOT_DIR}'..."
    mkdir -p "${ROOT_DIR}/sbin" "${ROOT_DIR}/bin" "${ROOT_DIR}/etc"
fi

# Ensure output directory exists
OUTPUT_DIR="$(dirname "${OUTPUT_CPIO}")"
mkdir -p "${OUTPUT_DIR}"

OUTPUT_CPIO_ABS="$(cd "${OUTPUT_DIR}" && pwd)/$(basename "${OUTPUT_CPIO}")"

echo "[INFO] Packaging '${ROOT_DIR}' into '${OUTPUT_CPIO}'..."

if ! command -v cpio &>/dev/null; then
    echo "[ERROR] 'cpio' command not found. Please install cpio (e.g. sudo apt install cpio)." >&2
    exit 1
fi

(
    cd "${ROOT_DIR}"
    find . -mindepth 1 ! -name 'st*' | sort | cpio -o -H newc -R 0:0 > "${OUTPUT_CPIO_ABS}.tmp"
    mv "${OUTPUT_CPIO_ABS}.tmp" "${OUTPUT_CPIO_ABS}"
)

echo "✔ [INFO] Generated ${OUTPUT_CPIO} ($(wc -c < "${OUTPUT_CPIO}") bytes)"
