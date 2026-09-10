#!/usr/bin/env bash
# ==============================================================================
# PetraOS Initramfs Builder
# ==============================================================================
# Synchronizes base-files and the xbstrap sysroot into initramfs root and
# packages it into a bootable SVR4 (newc) CPIO archive.
# ==============================================================================

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASE_FILES="${REPO_ROOT}/base-files"

SYNC_ONLY=0
if [ "${1:-}" = "--sync-only" ]; then
    SYNC_ONLY=1
    INITRAMFS_ROOT="${2:-${REPO_ROOT}/build/initramfs_root}"
    OUTPUT_CPIO="${REPO_ROOT}/build/initramfs.cpio"
    SYSROOT="${3:-${REPO_ROOT}/build-xbstrap/system-root}"
else
    INITRAMFS_ROOT="${1:-${REPO_ROOT}/build/initramfs_root}"
    OUTPUT_CPIO="${2:-${REPO_ROOT}/build/initramfs.cpio}"
    SYSROOT="${3:-${REPO_ROOT}/build-xbstrap/system-root}"
fi

# Ensure absolute paths so subshells changing directories (cd) don't break relative paths
[[ "${INITRAMFS_ROOT}" = /* ]] || INITRAMFS_ROOT="${REPO_ROOT}/${INITRAMFS_ROOT}"
[[ "${OUTPUT_CPIO}" = /* ]] || OUTPUT_CPIO="${REPO_ROOT}/${OUTPUT_CPIO}"
[[ "${SYSROOT}" = /* ]] || SYSROOT="${REPO_ROOT}/${SYSROOT}"

echo "[INFO] Syncing initramfs tree at '${INITRAMFS_ROOT}'..."
mkdir -p "${INITRAMFS_ROOT}"

# Remove legacy directories that clash with UsrMerge symlinks
for d in bin sbin lib lib64 usr/sbin usr/lib64 var/run; do
    if [ -d "${INITRAMFS_ROOT}/${d}" ] && [ ! -L "${INITRAMFS_ROOT}/${d}" ]; then
        rm -rf "${INITRAMFS_ROOT:?}/${d}"
    fi
done

# 1. Apply base-files (UsrMerge symlinks, /etc configs)
if [ -d "${BASE_FILES}" ]; then
    cp -a --remove-destination "${BASE_FILES}/." "${INITRAMFS_ROOT}/"
fi

# 2. Copy xbstrap sysroot packages
if [ -d "${SYSROOT}" ]; then
    cp -a --remove-destination "${SYSROOT}/." "${INITRAMFS_ROOT}/"
fi

# 3. Ensure default shell symlinks
ln -sf bash "${INITRAMFS_ROOT}/usr/bin/sh" 2>/dev/null || true
[ -e "${INITRAMFS_ROOT}/usr/bin/init" ] || ln -sf bash "${INITRAMFS_ROOT}/usr/bin/init" 2>/dev/null || true

# 4. Copy host terminfo (best-effort)
if [ -d /usr/share/terminfo ]; then
    mkdir -p "${INITRAMFS_ROOT}/usr/share/terminfo"
    cp -rn /usr/share/terminfo/* "${INITRAMFS_ROOT}/usr/share/terminfo/" 2>/dev/null || true
fi

echo "✔ [INFO] Synced initramfs root directory."

[ "${SYNC_ONLY}" -eq 1 ] && exit 0

# 5. Package into CPIO archive
echo "[INFO] Packaging into '${OUTPUT_CPIO}'..."
mkdir -p "$(dirname "${OUTPUT_CPIO}")"

(
    cd "${INITRAMFS_ROOT}"
    find . -mindepth 1 | sort | cpio -o -H newc -R 0:0 > "${OUTPUT_CPIO}.tmp"
    mv "${OUTPUT_CPIO}.tmp" "${OUTPUT_CPIO}"
)

echo "✔ [INFO] Generated ${OUTPUT_CPIO} ($(wc -c < "${OUTPUT_CPIO}") bytes)"
