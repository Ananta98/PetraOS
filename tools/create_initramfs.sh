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

# Helper: copy directory tree (mirrors original `cp -rf src/* dst/` behaviour, DRY).
copy_tree() {
    local src="$1" dst="$2"
    [ -d "${src}" ] || return 0
    mkdir -p "${dst}"
    cp -rf "${src}/"* "${dst}/" 2>/dev/null || true
}

sync_ramfs() {
    local root_dir="${1:-${DEFAULT_ROOT_DIR}}"
    local sysroot="${2:-${DEFAULT_SYSROOT}}"

    echo "[INFO] Syncing sysroot to initramfs root..."

    # 1. Create minimal FHS hierarchy (POSIX + Linux conventions)
    mkdir -p "${root_dir}/bin" "${root_dir}/sbin" "${root_dir}/lib" "${root_dir}/libexec" \
             "${root_dir}/usr/bin" "${root_dir}/usr/lib" "${root_dir}/usr/sbin" "${root_dir}/usr/libexec" \
             "${root_dir}/usr/include" "${root_dir}/usr/share" "${root_dir}/etc" "${root_dir}/tmp" "${root_dir}/var/tmp" \
             "${root_dir}/include"

    # 2. Sync sysroot -> initramfs root
    if [ -d "${sysroot}" ]; then
        # Single-destination trees
        copy_tree "${sysroot}/bin"       "${root_dir}/bin"
        copy_tree "${sysroot}/sbin"      "${root_dir}/sbin"
        copy_tree "${sysroot}/lib"       "${root_dir}/lib"
        copy_tree "${sysroot}/etc"       "${root_dir}/etc"
        copy_tree "${sysroot}/usr/share" "${root_dir}/usr/share"
        copy_tree "${sysroot}/usr/etc"   "${root_dir}/etc"
        copy_tree "${sysroot}/usr/x86_64-petra" "${root_dir}/usr/x86_64-petra"

        # Dual-destination (mirrored) for /usr compat: /usr/* <=> /*
        local spec src dst1 dst2
        for spec in \
            "usr/bin:usr/bin:bin" \
            "usr/sbin:usr/sbin:sbin" \
            "usr/lib:usr/lib:lib" \
            "usr/libexec:usr/libexec:libexec" \
            "usr/include:usr/include:include"
        do
            IFS=':' read -r src dst1 dst2 <<< "${spec}"
            [ -d "${sysroot}/${src}" ] || continue
            copy_tree "${sysroot}/${src}" "${root_dir}/${dst1}"
            copy_tree "${sysroot}/${src}" "${root_dir}/${dst2}"
        done

        # Toolchain symlinks (as, ld) for direct invokers
        if [ -f "${root_dir}/usr/x86_64-petra/bin/as" ]; then
            ln -sf /usr/x86_64-petra/bin/as "${root_dir}/usr/bin/as" 2>/dev/null || true
            ln -sf /usr/x86_64-petra/bin/as "${root_dir}/bin/as" 2>/dev/null || true
        fi
        if [ -f "${root_dir}/usr/x86_64-petra/bin/ld" ]; then
            ln -sf /usr/x86_64-petra/bin/ld "${root_dir}/usr/bin/ld" 2>/dev/null || true
            ln -sf /usr/x86_64-petra/bin/ld "${root_dir}/bin/ld" 2>/dev/null || true
        fi
    fi

    # 3. Host terminfo for ncurses/readline (best-effort)
    if [ -d /usr/share/terminfo ]; then
        mkdir -p "${root_dir}/usr/share/terminfo" "${root_dir}/etc/terminfo"
        cp -rf /usr/share/terminfo/* "${root_dir}/usr/share/terminfo/" 2>/dev/null || true
        cp -rf /usr/share/terminfo/* "${root_dir}/etc/terminfo/" 2>/dev/null || true
    fi

    # 4. POSIX shell symlinks: Linux convention is /bin/sh -> bash
    if [ -f "${root_dir}/usr/bin/bash" ] || [ -f "${root_dir}/bin/bash" ]; then
        mkdir -p "${root_dir}/bin" "${root_dir}/usr/bin"
        ln -sf /bin/bash     "${root_dir}/bin/sh"     2>/dev/null || true
        ln -sf /usr/bin/bash "${root_dir}/usr/bin/sh" 2>/dev/null || true
        # Ensure bash exists at both canonical locations for compatibility
        if [ ! -f "${root_dir}/bin/bash" ] && [ -f "${root_dir}/usr/bin/bash" ]; then
            cp -a "${root_dir}/usr/bin/bash" "${root_dir}/bin/bash" 2>/dev/null \
                || ln -sf /usr/bin/bash "${root_dir}/bin/bash" 2>/dev/null || true
        fi
        if [ ! -f "${root_dir}/usr/bin/bash" ] && [ -f "${root_dir}/bin/bash" ]; then
            cp -a "${root_dir}/bin/bash" "${root_dir}/usr/bin/bash" 2>/dev/null \
                || ln -sf /bin/bash "${root_dir}/usr/bin/bash" 2>/dev/null || true
        fi
    fi

    # 5. Linux init fallbacks: /sbin/init, /bin/init, /etc/init -> /bin/sh
    # Ensures DEFAULT_INIT_EXEC_PATHS always resolves even when only bash is present.
    if [ -e "${root_dir}/bin/sh" ]; then
        for p in sbin/init bin/init etc/init; do
            if [ ! -e "${root_dir}/${p}" ]; then
                mkdir -p "$(dirname "${root_dir}/${p}")"
                ln -sf /bin/sh "${root_dir}/${p}" 2>/dev/null || true
            fi
        done
    fi

    # 5.5 Minimal user database for getpwuid/getgrgid (fixes whoami/id -un, bash \u).
    # mlibc reads /etc/passwd and /etc/group directly; without them
    # getpwuid(0) returns NULL and bash falls back to "I have no name!".
    mkdir -p "${root_dir}/etc"
    if [ ! -f "${root_dir}/etc/passwd" ]; then
        printf 'root:x:0:0:root:/:/bin/bash\n' > "${root_dir}/etc/passwd"
    elif ! grep -q '^root:' "${root_dir}/etc/passwd" 2>/dev/null; then
        printf 'root:x:0:0:root:/:/bin/bash\n' >> "${root_dir}/etc/passwd"
    fi
    if [ ! -f "${root_dir}/etc/group" ]; then
        printf 'root:x:0:\n' > "${root_dir}/etc/group"
    elif ! grep -q '^root:' "${root_dir}/etc/group" 2>/dev/null; then
        printf 'root:x:0:\n' >> "${root_dir}/etc/group"
    fi
    chmod 644 "${root_dir}/etc/passwd" "${root_dir}/etc/group" 2>/dev/null || true

    # 6. Strip debug symbols for size (best-effort, only ELF binaries)
    local strip_bin="x86_64-linux-gnu-strip"
    command -v "${strip_bin}" &>/dev/null || strip_bin="strip"
    if command -v "${strip_bin}" &>/dev/null; then
        for d in bin usr/bin usr/libexec; do
            if [ -d "${root_dir}/${d}" ]; then
                find "${root_dir}/${d}" -type f -exec sh -c '
                    for f; do
                        if file "$f" 2>/dev/null | grep -q "ELF"; then
                            '"${strip_bin}"' -s "$f" 2>/dev/null || true
                        fi
                    done
                ' _ {} +
            fi
        done
        for d in lib usr/lib; do
            [ -d "${root_dir}/${d}" ] && find "${root_dir}/${d}" -name "*.so*" -type f -exec "${strip_bin}" -s {} + 2>/dev/null || true
        done
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

    local output_dir output_cpio_abs
    output_dir="$(dirname "${output_cpio}")"
    mkdir -p "${output_dir}"
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
