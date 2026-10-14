#!/bin/bash
# ==============================================================================
# PetraOS Silica Kernel - Alpine Linux Minirootfs Initramfs Packager
# ==============================================================================
# This script prepares a complete Alpine Linux x86_64 userspace environment
# packaged as a CPIO newc archive suitable for booting PetraOS Silica.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

ALPINE_VERSION="3.20.3"
ALPINE_BRANCH="v3.20"
ALPINE_TAR_NAME="alpine-minirootfs-${ALPINE_VERSION}-x86_64.tar.gz"
ALPINE_URL="https://dl-cdn.alpinelinux.org/alpine/${ALPINE_BRANCH}/releases/x86_64/${ALPINE_TAR_NAME}"

CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/initramfs_base"
OUTPUT_CPIO="${REPO_ROOT}/build/initramfs.cpio"

mkdir -p "${CACHE_DIR}"
mkdir -p "$(dirname "${OUTPUT_CPIO}")"

echo "=== [1/6] Downloading Alpine Linux minirootfs ==="
if [ ! -f "${CACHE_DIR}/${ALPINE_TAR_NAME}" ]; then
    echo "Downloading ${ALPINE_TAR_NAME} from ${ALPINE_URL}..."
    curl -L --fail --retry 3 -o "${CACHE_DIR}/${ALPINE_TAR_NAME}" "${ALPINE_URL}"
else
    echo "Using cached archive: ${CACHE_DIR}/${ALPINE_TAR_NAME}"
fi

echo "=== [2/6] Extracting rootfs into ${BUILD_DIR} ==="
rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}"
tar -xzf "${CACHE_DIR}/${ALPINE_TAR_NAME}" -C "${BUILD_DIR}"

echo "=== [3/6] Setting up essential directories and permissions ==="
mkdir -p "${BUILD_DIR}"/{bin,sbin,lib,usr/bin,usr/sbin,usr/lib,usr/share}
mkdir -p "${BUILD_DIR}"/{dev,dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,media,etc/defaults,var/log,var/run,var/tmp}

chmod 1777 "${BUILD_DIR}/tmp" "${BUILD_DIR}/var/tmp"
chmod 0750 "${BUILD_DIR}/root"

echo "=== [4/6] Creating /init startup script ==="
cat << 'EOF' > "${BUILD_DIR}/init"
#!/bin/sh
# PetraOS Silica Kernel Userspace Init Script
# Executed as PID 1 upon kernel boot

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
export HOME=/root
export USER=root
export TERM=linux
export SHELL=/bin/sh

# Mount essential pseudo-filesystems
mount -t proc proc /proc 2>/dev/null || true
mount -t sysfs sysfs /sys 2>/dev/null || true
mount -t tmpfs tmpfs /tmp 2>/dev/null || true
mount -t tmpfs tmpfs /run 2>/dev/null || true
mkdir -p /dev/pts /dev/shm 2>/dev/null || true
mount -t devpts devpts /dev/pts 2>/dev/null || true
mount -t tmpfs tmpfs /dev/shm 2>/dev/null || true

# Set up standard device symlinks
ln -sf /proc/self/fd /dev/fd 2>/dev/null || true
ln -sf /proc/self/fd/0 /dev/stdin 2>/dev/null || true
ln -sf /proc/self/fd/1 /dev/stdout 2>/dev/null || true
ln -sf /proc/self/fd/2 /dev/stderr 2>/dev/null || true

# Display system banner
echo ""
echo "============================================================"
echo "    Welcome to PetraOS Silica Linux (Alpine Userspace)     "
echo "============================================================"
echo "Kernel: $(cat /proc/version 2>/dev/null || echo 'PetraOS Silica 6.6.0-silica x86_64')"
echo "Release: Alpine Linux $(cat /etc/alpine-release 2>/dev/null || echo '3.20.3')"
echo "Uptime: $(cat /proc/uptime 2>/dev/null | awk '{print $1}' || echo '0')s"
echo "Architecture: $(uname -m 2>/dev/null || echo 'x86_64')"
echo "============================================================"
echo ""

# Execute custom command if passed as arguments, else spawn login shell
if [ $# -gt 0 ] && [ "$1" != "sh" ]; then
    echo "[init] Executing requested command: $@"
    exec "$@"
fi

echo "[init] Spawning interactive Alpine login shell (/bin/sh -l)..."
exec /bin/sh -l
EOF
chmod 0755 "${BUILD_DIR}/init"

echo "=== [5/6] Creating standard system configuration files ==="

# User & group configuration
cat << 'EOF' > "${BUILD_DIR}/etc/passwd"
root:x:0:0:root:/root:/bin/sh
bin:x:1:1:bin:/bin:/sbin/nologin
daemon:x:2:2:daemon:/sbin:/sbin/nologin
nobody:x:65534:65534:nobody:/:/sbin/nologin
EOF

cat << 'EOF' > "${BUILD_DIR}/etc/group"
root:x:0:root
bin:x:1:root,bin,daemon
daemon:x:2:root,bin,daemon
sys:x:3:root,bin,adm
adm:x:4:root,adm,daemon
tty:x:5:
disk:x:6:root
wheel:x:10:root
audio:x:29:
video:x:44:
users:x:100:
nobody:x:65534:
EOF

# Network configuration
cat << 'EOF' > "${BUILD_DIR}/etc/hosts"
127.0.0.1 localhost petraos
::1       localhost ip6-localhost ip6-loopback
EOF

cat << 'EOF' > "${BUILD_DIR}/etc/resolv.conf"
nameserver 10.0.2.3
nameserver 1.1.1.1
nameserver 8.8.8.8
EOF

cat << 'EOF' > "${BUILD_DIR}/etc/hostname"
petraos
EOF

# Shell profile
cat << 'EOF' > "${BUILD_DIR}/root/.profile"
export PS1='petraos:\w# '
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
export TERM=linux
alias ll='ls -la --color=auto'
alias l='ls -lh'
EOF

cat << 'EOF' > "${BUILD_DIR}/etc/profile"
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
export TERM=linux
export PS1='petraos:\w# '
umask 022
EOF

# Compile userspace keyboard IOCTL test binary
if [ -f "${REPO_ROOT}/tools/test_keyboard_ioctl.c" ] && command -v gcc >/dev/null 2>&1; then
    echo "Compiling userspace keyboard ioctl test suite..."
    gcc -static -O2 -Wall "${REPO_ROOT}/tools/test_keyboard_ioctl.c" -o "${BUILD_DIR}/bin/test_keyboard_ioctl"
    chmod 0755 "${BUILD_DIR}/bin/test_keyboard_ioctl"
fi

# Optional device nodes
mknod -m 666 "${BUILD_DIR}/dev/null" c 1 3 2>/dev/null || true
mknod -m 666 "${BUILD_DIR}/dev/zero" c 1 5 2>/dev/null || true
mknod -m 666 "${BUILD_DIR}/dev/tty" c 5 0 2>/dev/null || true
mknod -m 600 "${BUILD_DIR}/dev/console" c 5 1 2>/dev/null || true

echo "=== [6/6] Generating CPIO newc archive ==="
cd "${BUILD_DIR}"
find . -mindepth 1 | sort | cpio -o -H newc > "${OUTPUT_CPIO}"

CPIO_SIZE=$(ls -lh "${OUTPUT_CPIO}" | awk '{print $5}')
ENTRY_COUNT=$(wc -l < <(find . -mindepth 1))

echo ""
echo "============================================================"
echo " Initramfs generated successfully!"
echo " Location:    ${OUTPUT_CPIO}"
echo " Size:        ${CPIO_SIZE}"
echo " Total files: ${ENTRY_COUNT} entries"
echo "============================================================"