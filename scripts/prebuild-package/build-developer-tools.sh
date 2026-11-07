#!/bin/bash
# Stage the PetraOS developer-tools layer: installs the complete CLI environment
# and development toolchain into the target root using Void Linux official xbps-static.
#
# Packages, dependencies, file checksums, and metadata are cleanly registered in
# /var/db/xbps, enabling native xbps usage inside PetraOS.
#
# Usage: build-developer-tools.sh [--root <target_root>]
#
# Environment:
#   VOID_MIRROR                  override the Void Linux repository mirror
#   PETRAOS_DEVELOPER_PACKAGES   space separated root package set
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

VOID_MIRROR="${VOID_MIRROR:-https://repo-default.voidlinux.org/current}"
XBPS_STATIC_URL="https://repo-default.voidlinux.org/static/xbps-static-latest.x86_64-musl.tar.xz"

CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/prebuild-package/developer-tools"
STAGING="${BUILD_DIR}/staging"

XBPS_STATIC_DIR="${CACHE_DIR}/xbps_static"
XBPS_INSTALL="${XBPS_STATIC_DIR}/usr/bin/xbps-install.static"

TARGET_ROOT="${STAGING}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --root)
            TARGET_ROOT="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [--root <dir>]"
            exit 0
            ;;
        *)
            echo "ERROR: unknown option: $1" >&2
            exit 1
            ;;
    esac
done

ROOT_PACKAGES=(
    bash
    binutils
    btop
    ca-certificates
    cmake
    coreutils
    diffutils
    e2fsprogs
    exfatprogs
    file
    findutils
    gawk
    gcc
    gdb
    git
    grep
    htop
    make
    meson
    ninja
    openssl
    pkgconf
    python3
    sed
    strace
    tar
    tmux
    vim
    which
    xz
)
root_packages="${PETRAOS_DEVELOPER_PACKAGES:-${ROOT_PACKAGES[*]}}"

for tool in curl tar xz; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "ERROR: required command not found: $tool" >&2
        exit 1
    }
done

bootstrap_xbps_static() {
    if [ -x "${XBPS_INSTALL}" ]; then
        return
    fi

    local archive="${CACHE_DIR}/xbps-static-latest.x86_64-musl.tar.xz"
    echo "==> Fetching xbps-static-latest"
    if [ ! -s "${archive}" ]; then
        curl -fsSL --retry 3 "${XBPS_STATIC_URL}" -o "${archive}"
    fi

    mkdir -p "${XBPS_STATIC_DIR}"
    tar -xJf "${archive}" -C "${XBPS_STATIC_DIR}"
    chmod +x "${XBPS_STATIC_DIR}"/usr/bin/* || true
}

mkdir -p "${CACHE_DIR}" "${BUILD_DIR}"
bootstrap_xbps_static

mkdir -p "${TARGET_ROOT}"
mkdir -p "${TARGET_ROOT}/var/db/xbps/keys"

# Copy Void Linux repository signature keys
if [ -d "${XBPS_STATIC_DIR}/var/db/xbps/keys" ]; then
    cp -a "${XBPS_STATIC_DIR}/var/db/xbps/keys/." "${TARGET_ROOT}/var/db/xbps/keys/"
fi

mkdir -p "${CACHE_DIR}/xbps-pkgs"

echo "==> Installing developer-tools into ${TARGET_ROOT}"
"${XBPS_INSTALL}" \
    -r "${TARGET_ROOT}" \
    -R "${VOID_MIRROR}" \
    -c "${CACHE_DIR}/xbps-pkgs" \
    -S -u -y --ignore-conf-repos \
    ${root_packages}

# Sanity checking: key promised tools must exist in the target root.
echo "==> Verifying installed developer tools"
check_binary() {
    local name="$1" dir rel
    for dir in usr/bin bin usr/sbin sbin; do
        rel="${dir}/${name}"
        if [ -e "${TARGET_ROOT}/${rel}" ] || [ -L "${TARGET_ROOT}/${rel}" ]; then
            echo "    ${rel}"
            return 0
        fi
    done
    echo "ERROR: missing installed binary: ${name}" >&2
    exit 1
}

for binary in bash gawk grep sed htop gcc make cmake \
    gdb git meson ninja pkg-config mkfs.ext4 mkfs.exfat openssl; do
    check_binary "${binary}"
done

if [ ! -e "${TARGET_ROOT}/usr/lib/ld-linux-x86-64.so.2" ] && [ ! -e "${TARGET_ROOT}/lib64/ld-linux-x86-64.so.2" ]; then
    echo "ERROR: missing glibc loader ld-linux-x86-64.so.2" >&2
    exit 1
fi

# Ensure all staged files are user-writable for subsequent merge steps
chmod -R u+w "${TARGET_ROOT}" 2>/dev/null || true

echo "    developer-tools successfully staged in ${TARGET_ROOT} ($(du -sh "${TARGET_ROOT}" | cut -f1))"
