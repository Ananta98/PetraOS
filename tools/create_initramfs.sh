#!/usr/bin/env bash
# ==============================================================================
# PetraOS Initramfs Creation & Synchronization Tool
#
# Synchronizes the xbstrap sysroot tree into the initramfs root directory
# and packages it into a standard SVR4 portable format (newc) CPIO archive.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

DEFAULT_ROOT_DIR="${REPO_ROOT}/build/initramfs_root"
DEFAULT_OUTPUT_CPIO="${REPO_ROOT}/build/initramfs.cpio"
DEFAULT_SYSROOT="${REPO_ROOT}/build-xbstrap/system-root"

sync_ramfs() {
    local root_dir="${1:-${DEFAULT_ROOT_DIR}}"
    local sysroot="${2:-${DEFAULT_SYSROOT}}"

    echo "[INFO] Syncing sysroot to initramfs root..."
    mkdir -p "${root_dir}/bin" "${root_dir}/sbin" "${root_dir}/lib" "${root_dir}/libexec" \
             "${root_dir}/usr/bin" "${root_dir}/usr/lib" "${root_dir}/usr/sbin" "${root_dir}/usr/libexec" \
             "${root_dir}/usr/include" "${root_dir}/usr/share" "${root_dir}/etc" "${root_dir}/tmp" "${root_dir}/var/tmp"

    if [ -d "${sysroot}" ]; then
        if [ -d "${sysroot}/lib" ]; then
            cp -rf "${sysroot}/lib/"* "${root_dir}/lib/" 2>/dev/null || true
        fi
        if [ -d "${sysroot}/usr/lib" ]; then
            cp -rf "${sysroot}/usr/lib/"* "${root_dir}/usr/lib/" 2>/dev/null || true
            cp -rf "${sysroot}/usr/lib/"* "${root_dir}/lib/" 2>/dev/null || true
        fi
        if [ -d "${sysroot}/usr/libexec" ]; then
            cp -rf "${sysroot}/usr/libexec/"* "${root_dir}/usr/libexec/" 2>/dev/null || true
            cp -rf "${sysroot}/usr/libexec/"* "${root_dir}/libexec/" 2>/dev/null || true
        fi
        if [ -d "${sysroot}/usr/include" ]; then
            cp -rf "${sysroot}/usr/include/"* "${root_dir}/usr/include/" 2>/dev/null || true
            cp -rf "${sysroot}/usr/include/"* "${root_dir}/include/" 2>/dev/null || true
        fi
        if [ -d "${sysroot}/usr/share" ]; then
            cp -rf "${sysroot}/usr/share/"* "${root_dir}/usr/share/" 2>/dev/null || true
        fi
        if [ -d "${sysroot}/bin" ]; then
            cp -rf "${sysroot}/bin/"* "${root_dir}/bin/" 2>/dev/null || true
        fi
        if [ -d "${sysroot}/sbin" ]; then
            cp -rf "${sysroot}/sbin/"* "${root_dir}/sbin/" 2>/dev/null || true
        fi
        if [ -d "${sysroot}/usr/bin" ]; then
            cp -rf "${sysroot}/usr/bin/"* "${root_dir}/usr/bin/" 2>/dev/null || true
            cp -rf "${sysroot}/usr/bin/"* "${root_dir}/bin/" 2>/dev/null || true
        fi
        if [ -d "${sysroot}/usr/sbin" ]; then
            cp -rf "${sysroot}/usr/sbin/"* "${root_dir}/usr/sbin/" 2>/dev/null || true
            cp -rf "${sysroot}/usr/sbin/"* "${root_dir}/sbin/" 2>/dev/null || true
        fi
        if [ -d "${sysroot}/etc" ]; then
            cp -rf "${sysroot}/etc/"* "${root_dir}/etc/" 2>/dev/null || true
        fi
    fi

    if [ -d /usr/share/terminfo ]; then
        mkdir -p "${root_dir}/usr/share/terminfo" "${root_dir}/etc/terminfo"
        cp -rf /usr/share/terminfo/* "${root_dir}/usr/share/terminfo/" 2>/dev/null || true
        cp -rf /usr/share/terminfo/* "${root_dir}/etc/terminfo/" 2>/dev/null || true
    fi

    if [ -f "${root_dir}/usr/bin/bash" ] || [ -f "${root_dir}/bin/bash" ]; then
        ln -sf /bin/bash "${root_dir}/bin/sh" 2>/dev/null || true
        ln -sf /usr/bin/bash "${root_dir}/usr/bin/sh" 2>/dev/null || true
    fi

    local STRIP_CMD="x86_64-linux-gnu-strip"
    if ! command -v "${STRIP_CMD}" &>/dev/null; then
        STRIP_CMD="strip"
    fi

    if command -v "${STRIP_CMD}" &>/dev/null; then
        if [ -d "${root_dir}/bin" ]; then
            find "${root_dir}/bin" "${root_dir}/usr/bin" "${root_dir}/usr/libexec" -type f -exec "${STRIP_CMD}" -s {} + 2>/dev/null || true
        fi
        if [ -d "${root_dir}/lib" ]; then
            find "${root_dir}/lib" "${root_dir}/usr/lib" -name "*.so*" -type f -exec "${STRIP_CMD}" -s {} + 2>/dev/null || true
        fi
    fi

    echo "✔ [INFO] Synced ramfs directory '${root_dir}'."
}

package_cpio() {
    local root_dir="${1:-${DEFAULT_ROOT_DIR}}"
    local output_cpio="${2:-${DEFAULT_OUTPUT_CPIO}}"

    if [ ! -d "${root_dir}" ]; then
        echo "[INFO] Creating directory tree '${root_dir}'..."
        mkdir -p "${root_dir}/sbin" "${root_dir}/bin" "${root_dir}/etc" "${root_dir}/tmp" "${root_dir}/var/tmp"
    fi

    local output_dir
    output_dir="$(dirname "${output_cpio}")"
    mkdir -p "${output_dir}"

    local output_cpio_abs
    output_cpio_abs="$(cd "${output_dir}" && pwd)/$(basename "${output_cpio}")"

    echo "[INFO] Packaging '${root_dir}' into '${output_cpio}'..."

    if ! command -v cpio &>/dev/null; then
        echo "[ERROR] 'cpio' command not found. Please install cpio (e.g. sudo apt install cpio)." >&2
        exit 1
    fi

    (
        cd "${root_dir}"
        find . -mindepth 1 | sort | cpio -o -H newc -R 0:0 > "${output_cpio_abs}.tmp"
        mv "${output_cpio_abs}.tmp" "${output_cpio_abs}"
    )

    echo "✔ [INFO] Generated ${output_cpio} ($(wc -c < "${output_cpio}") bytes)"
}

show_help() {
    echo "Usage: $0 [mode|root_dir] [output_cpio] [sysroot]"
    echo ""
    echo "Modes:"
    echo "  --sync-only, sync, sync_ramfs [root_dir] [sysroot]   Sync sysroot into initramfs root only"
    echo "  --package-only, package [root_dir] [output_cpio]    Package initramfs root into cpio archive only"
    echo "  (default) [root_dir] [output_cpio] [sysroot]         Sync sysroot and package cpio archive"
}

MODE="${1:-}"

case "${MODE}" in
    --sync-only|sync|sync_ramfs|--sync)
        ROOT_DIR="${2:-${DEFAULT_ROOT_DIR}}"
        SYSROOT="${3:-${DEFAULT_SYSROOT}}"
        sync_ramfs "${ROOT_DIR}" "${SYSROOT}"
        ;;
    --package-only|package)
        ROOT_DIR="${2:-${DEFAULT_ROOT_DIR}}"
        OUTPUT_CPIO="${3:-${DEFAULT_OUTPUT_CPIO}}"
        package_cpio "${ROOT_DIR}" "${OUTPUT_CPIO}"
        ;;
    --help|-h|help)
        show_help
        ;;
    *)
        ROOT_DIR="${1:-${DEFAULT_ROOT_DIR}}"
        OUTPUT_CPIO="${2:-${DEFAULT_OUTPUT_CPIO}}"
        SYSROOT="${3:-${DEFAULT_SYSROOT}}"
        sync_ramfs "${ROOT_DIR}" "${SYSROOT}"
        package_cpio "${ROOT_DIR}" "${OUTPUT_CPIO}"
        ;;
esac
