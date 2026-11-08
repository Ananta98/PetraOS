#!/bin/bash
# Assemble the PetraOS amd64 userland:
#   1. A lightweight in-memory boot initramfs (build/userland-amd64/staging)
#      containing busybox and the /init orchestrator (< 2MB).
#   2. A complete Void Linux glibc userspace (build/userland-amd64/rootfs_staging)
#      populated from void-x86_64-ROOTFS and prebuilt developer-tools, packaged
#      into the persistent ext4 disk image (build/ext4.img).
#
# Usage: build-userland-amd64.sh [--fresh] [--purge-cache]
#
#   --fresh        remove existing userland artifacts before rebuilding
#   --purge-cache  with --fresh, also drop the download cache and re-fetch Void Linux
#
# Environment:
#   PETRAOS_USERLAND_PACKAGES  space separated prebuild-package layer names
#                              (default: developer-tools)
#   PETRAOS_VERSION            version string reported by /etc/os-release
#   VOID_VERSION/VOID_SHA256   override the Void Linux release
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

VOID_VERSION="${VOID_VERSION:-20250202}"
VOID_ARCH="x86_64"
VOID_SHA256="${VOID_SHA256:-3f48e6673ac5907a897d913c97eb96edbfb230162731b4016562c51b3b8f1876}"
VOID_TAR_NAME="void-${VOID_ARCH}-ROOTFS-${VOID_VERSION}.tar.xz"
VOID_MIRROR="${VOID_MIRROR:-https://repo-default.voidlinux.org/live/current}"

BUSYBOX_VERSION="1.35.0"
BUSYBOX_URL="https://busybox.net/downloads/binaries/${BUSYBOX_VERSION}-x86_64-linux-musl/busybox"
BUSYBOX_SHA256="6e123e7f3202a8c1e9b1f94d8941580a25135382b99e8d3e34fb858bba311348"
BUSYBOX_BIN="busybox-x86_64-${BUSYBOX_VERSION}"

CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/userland-amd64"
STAGING="${BUILD_DIR}/staging"
ROOTFS_STAGING="${BUILD_DIR}/rootfs_staging"
INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"

PACKAGES="${PETRAOS_USERLAND_PACKAGES:-developer-tools}"
PETRAOS_VERSION="${PETRAOS_VERSION:-0.1.0}"
SUPPORT_DIR="${REPO_ROOT}/scripts/build-support"
PREBUILD_DIR="${REPO_ROOT}/scripts/prebuild-package"
BASE_FILES_DIR="${REPO_ROOT}/base-files"

FRESH=0
PURGE_CACHE=0

for arg in "$@"; do
    case "$arg" in
        --fresh) FRESH=1 ;;
        --purge-cache) PURGE_CACHE=1 ;;
        -h|--help)
            sed -n '2,/^set -/s/^# \{0,1\}//p' "$0"
            exit 0
            ;;
        *)
            echo "ERROR: unknown option: $arg" >&2
            exit 1
            ;;
    esac
done

if [ "${FRESH}" -eq 1 ]; then
    clean_args=(--userland)
    [ "${PURGE_CACHE}" -eq 1 ] && clean_args+=(--cache)
    "${SCRIPT_DIR}/../clean.sh" "${clean_args[@]}"
fi

for tool in curl tar xz file; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "ERROR: required command not found: $tool" >&2
        exit 1
    }
done

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

mkdir -p "${CACHE_DIR}" "${BUILD_DIR}"

# ----------------------------------------------------------------------------
# 1. Fetch Downloads (Cached)
# ----------------------------------------------------------------------------

# A. Static BusyBox for lightweight initramfs
if [ ! -f "${CACHE_DIR}/${BUSYBOX_BIN}" ]; then
    echo "==> Fetching static BusyBox ${BUSYBOX_VERSION}..."
    curl -fL --retry 3 "${BUSYBOX_URL}" -o "${CACHE_DIR}/${BUSYBOX_BIN}"
fi

actual_bb_sha256="$(sha256_file "${CACHE_DIR}/${BUSYBOX_BIN}")"
if [ "${actual_bb_sha256}" != "${BUSYBOX_SHA256}" ]; then
    echo "ERROR: BusyBox checksum mismatch." >&2
    echo "Expected: ${BUSYBOX_SHA256}" >&2
    echo "Actual:   ${actual_bb_sha256}" >&2
    exit 1
fi
chmod +x "${CACHE_DIR}/${BUSYBOX_BIN}"

# B. Void Linux Base Rootfs for ext4 block image
if [ ! -f "${CACHE_DIR}/${VOID_TAR_NAME}" ]; then
    echo "==> Fetching Void Linux ${VOID_VERSION} ${VOID_ARCH} rootfs..."
    curl -fL --retry 3 "${VOID_MIRROR}/${VOID_TAR_NAME}" -o "${CACHE_DIR}/${VOID_TAR_NAME}"
fi

actual_void_sha256="$(sha256_file "${CACHE_DIR}/${VOID_TAR_NAME}")"
if [ "${actual_void_sha256}" != "${VOID_SHA256}" ]; then
    echo "ERROR: Void Linux rootfs checksum mismatch." >&2
    echo "Expected: ${VOID_SHA256}" >&2
    echo "Actual:   ${actual_void_sha256}" >&2
    exit 1
fi

# ----------------------------------------------------------------------------
# 2. Assemble Lightweight Initramfs (STAGING)
# ----------------------------------------------------------------------------
echo "==> Building lightweight in-memory initramfs in ${STAGING}"
rm -rf "${STAGING}"
mkdir -p "${STAGING}"/{bin,sbin,dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,sysroot,etc}
chmod 1777 "${STAGING}/tmp"
chmod 0750 "${STAGING}/root"

# Install static busybox and core symlinks
cp -f "${CACHE_DIR}/${BUSYBOX_BIN}" "${STAGING}/bin/busybox"
chmod 0755 "${STAGING}/bin/busybox"

for applet in sh ash mount umount mkdir mknod echo cat ls chroot switch_root \
              cp mv rm ln test kill ps sleep sync blkid dmesg uname; do
    ln -sf busybox "${STAGING}/bin/${applet}"
done
ln -sf bin/busybox "${STAGING}/sbin/mount" 2>/dev/null || true
ln -sf ../bin/busybox "${STAGING}/sbin/mke2fs" 2>/dev/null || true

# Install init orchestrator
install -m0755 "${SUPPORT_DIR}/init" "${STAGING}/init"

# Essential device nodes in initramfs
mknod -m 666 "${STAGING}/dev/null" c 1 3 2>/dev/null || true
mknod -m 666 "${STAGING}/dev/zero" c 1 5 2>/dev/null || true
mknod -m 666 "${STAGING}/dev/tty" c 5 0 2>/dev/null || true
mknod -m 600 "${STAGING}/dev/console" c 5 1 2>/dev/null || true
mknod -m 660 "${STAGING}/dev/sda" b 8 0 2>/dev/null || true
mknod -m 660 "${STAGING}/dev/sdb" b 8 16 2>/dev/null || true
mknod -m 660 "${STAGING}/dev/vda" b 254 0 2>/dev/null || true

# Install initramfs base-files skeleton
cp -a "${BASE_FILES_DIR}/initramfs/." "${STAGING}/"
printf '%s\n' "${PETRAOS_VERSION}" > "${STAGING}/etc/petraos-release"

# ----------------------------------------------------------------------------
# 3. Assemble Full Persistent Rootfs (ROOTFS_STAGING) for ext4.img
# ----------------------------------------------------------------------------
echo "==> Extracting full Void Linux userspace into ${ROOTFS_STAGING}"
rm -rf "${ROOTFS_STAGING}"
mkdir -p "${ROOTFS_STAGING}"
tar -xf "${CACHE_DIR}/${VOID_TAR_NAME}" -C "${ROOTFS_STAGING}"
chmod -R u+w "${ROOTFS_STAGING}" 2>/dev/null || true

echo "==> Staging prebuilt package layers into rootfs (${ROOTFS_STAGING})"
for package in ${PACKAGES}; do
    layer_script="${PREBUILD_DIR}/build-${package}.sh"
    if [ ! -x "${layer_script}" ]; then
        echo "ERROR: no prebuild-package script for '${package}': ${layer_script}" >&2
        exit 1
    fi
    echo "    ${package}"
    "${layer_script}" --root "${ROOTFS_STAGING}"
done

echo "==> Configuring full Void Linux rootfs"
mkdir -p "${ROOTFS_STAGING}"/{dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,media,var/log,var/run,var/tmp,sysroot}
chmod 1777 "${ROOTFS_STAGING}/tmp" "${ROOTFS_STAGING}/var/tmp"
chmod 0750 "${ROOTFS_STAGING}/root"
install -m0755 "${SUPPORT_DIR}/init" "${ROOTFS_STAGING}/init"

# Ensure /usr/bin/python symlink points to python3 if python3 is present
if [ -x "${ROOTFS_STAGING}/usr/bin/python3" ] && [ ! -e "${ROOTFS_STAGING}/usr/bin/python" ]; then
    ln -sfn python3 "${ROOTFS_STAGING}/usr/bin/python"
fi

# Pre-generate ld.so.cache if ldconfig is available
if [ -x "${ROOTFS_STAGING}/sbin/ldconfig" ] || [ -x "${ROOTFS_STAGING}/usr/bin/ldconfig" ]; then
    if [ "$(id -u)" -ne 0 ] && command -v unshare >/dev/null 2>&1; then
        unshare -r chroot "${ROOTFS_STAGING}" ldconfig 2>/dev/null || true
    fi
fi

# Deploy full rootfs base-files skeleton
echo "==> Deploying base-files skeleton into rootfs (${ROOTFS_STAGING})"
cp -a "${BASE_FILES_DIR}/rootfs/." "${ROOTFS_STAGING}/"
chmod 0600 "${ROOTFS_STAGING}/etc/shadow"
chmod 0750 "${ROOTFS_STAGING}/root"
mkdir -p "${ROOTFS_STAGING}/root/.config"
chmod 0750 "${ROOTFS_STAGING}/root/.config"

# Configure Void Linux runit services in /etc/runit/runsvdir/default
mkdir -p "${ROOTFS_STAGING}/etc/runit/runsvdir/default"

# Remove unsupported virtual terminal and hardware-dependent services
rm -f "${ROOTFS_STAGING}/etc/runit/runsvdir/default/agetty-tty2" \
      "${ROOTFS_STAGING}/etc/runit/runsvdir/default/agetty-tty3" \
      "${ROOTFS_STAGING}/etc/runit/runsvdir/default/agetty-tty4" \
      "${ROOTFS_STAGING}/etc/runit/runsvdir/default/agetty-tty5" \
      "${ROOTFS_STAGING}/etc/runit/runsvdir/default/agetty-tty6" \
      "${ROOTFS_STAGING}/etc/runit/runsvdir/default/udevd"

# Enable agetty-console with automatic root login
ln -sfn /etc/sv/agetty-console "${ROOTFS_STAGING}/etc/runit/runsvdir/default/agetty-console"

# Ensure /var/service points to /run/runit/runsvdir/current
rm -rf "${ROOTFS_STAGING}/var/service"
ln -sfn ../run/runit/runsvdir/current "${ROOTFS_STAGING}/var/service"

# Sync version info in os-release and petraos-release
sed -i "s/VERSION_ID=.*/VERSION_ID=\"${PETRAOS_VERSION}\"/" "${ROOTFS_STAGING}/usr/lib/os-release"
sed -i "s/VERSION=.*/VERSION=\"${PETRAOS_VERSION}\"/" "${ROOTFS_STAGING}/usr/lib/os-release"
rm -f "${ROOTFS_STAGING}/etc/os-release"
ln -sf ../usr/lib/os-release "${ROOTFS_STAGING}/etc/os-release"
printf '%s\n' "${PETRAOS_VERSION}" > "${ROOTFS_STAGING}/etc/petraos-release"

mknod -m 666 "${ROOTFS_STAGING}/dev/null" c 1 3 2>/dev/null || true
mknod -m 666 "${ROOTFS_STAGING}/dev/zero" c 1 5 2>/dev/null || true
mknod -m 666 "${ROOTFS_STAGING}/dev/tty" c 5 0 2>/dev/null || true
mknod -m 600 "${ROOTFS_STAGING}/dev/console" c 5 1 2>/dev/null || true
mknod -m 660 "${ROOTFS_STAGING}/dev/sda" b 8 0 2>/dev/null || true
mknod -m 660 "${ROOTFS_STAGING}/dev/sdb" b 8 16 2>/dev/null || true
mknod -m 660 "${ROOTFS_STAGING}/dev/vda" b 254 0 2>/dev/null || true

# ----------------------------------------------------------------------------
# 4. Packaging
# ----------------------------------------------------------------------------
echo "==> Packaging initramfs and ext4 disk images"
"${SCRIPT_DIR}/../build-initramfs.sh" \
    --staging "${STAGING}" \
    --rootfs-staging "${ROOTFS_STAGING}" \
    --output "${INITRAMFS}"

echo "==> Status:"
echo "    Initramfs size: $(du -h "${INITRAMFS}" | cut -f1)"
echo "    Ext4 image size: $(du -h "${REPO_ROOT}/build/ext4.img" | cut -f1)"
