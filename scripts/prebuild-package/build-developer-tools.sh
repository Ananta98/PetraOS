#!/bin/bash
# Stage the PetraOS developer-tools layer: installs the complete CLI environment
# and development toolchain into the target root using Alpine's official apk.static.
#
# Packages, dependencies, file checksums, and metadata are cleanly registered in
# /lib/apk/db/installed and /etc/apk/world, preventing duplicate downloads and
# unneeded package purging when apk is later used inside PetraOS.
#
# Usage: build-developer-tools.sh [--root <target_root>]
#
# Environment:
#   ALPINE_BRANCH/ALPINE_MIRROR  override the Alpine release/mirror
#   PETRAOS_DEVELOPER_PACKAGES   space separated root package set
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

ALPINE_BRANCH="${ALPINE_BRANCH:-v3.20}"
ALPINE_ARCH="${ALPINE_ARCH:-x86_64}"
ALPINE_MIRROR="${ALPINE_MIRROR:-https://dl-cdn.alpinelinux.org/alpine}"
APK_TOOLS_VERSION="${APK_TOOLS_VERSION:-2.14.4-r1}"

CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/prebuild-package/developer-tools"
STAGING="${BUILD_DIR}/staging"
APK_STATIC="${CACHE_DIR}/apk.static"

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
    autoconf
    automake
    bash
    btop
    build-base
    ca-certificates-bundle
    cmake
    coreutils
    diffutils
    e2fsprogs
    exfatprogs
    fastfetch
    file
    findutils
    gawk
    gdb
    git
    grep
    htop
    libtool
    m4
    make
    meson
    openssl
    patch
    pkgconf
    samurai
    sed
    strace
    tar
    tmux
    xz
)
root_packages="${PETRAOS_DEVELOPER_PACKAGES:-${ROOT_PACKAGES[*]}}"

for tool in curl tar; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "ERROR: required command not found: $tool" >&2
        exit 1
    }
done

bootstrap_apk_static() {
    if [ -x "${APK_STATIC}" ]; then
        return
    fi

    local archive tmp
    archive="${CACHE_DIR}/apk-tools-static-${APK_TOOLS_VERSION}.apk"
    tmp="$(mktemp "${CACHE_DIR}/.apk.static.XXXXXX")"
    trap 'rm -f "${tmp}"' RETURN

    echo "==> Fetching apk-tools-static ${APK_TOOLS_VERSION}"
    if [ ! -s "${archive}" ]; then
        curl -fsSL --retry 3 \
            "${ALPINE_MIRROR}/${ALPINE_BRANCH}/main/${ALPINE_ARCH}/apk-tools-static-${APK_TOOLS_VERSION}.apk" \
            -o "${archive}"
    fi
    tar xOf "${archive}" sbin/apk.static > "${tmp}"
    chmod 0755 "${tmp}"
    mv -f "${tmp}" "${APK_STATIC}"
}

mkdir -p "${CACHE_DIR}" "${BUILD_DIR}"
bootstrap_apk_static

mkdir -p "${TARGET_ROOT}"
mkdir -p "${TARGET_ROOT}/etc/apk"

# Ensure repositories file exists
cat > "${TARGET_ROOT}/etc/apk/repositories" << EOF
${ALPINE_MIRROR}/${ALPINE_BRANCH}/main
${ALPINE_MIRROR}/${ALPINE_BRANCH}/community
EOF

EXTRA_FLAGS=()
if [ ! -f "${TARGET_ROOT}/lib/apk/db/installed" ]; then
    EXTRA_FLAGS+=(--initdb)
fi

echo "==> Installing developer-tools into ${TARGET_ROOT}"
"${APK_STATIC}" \
    --root "${TARGET_ROOT}" \
    "${EXTRA_FLAGS[@]}" \
    --arch "${ALPINE_ARCH}" \
    --allow-untrusted \
    --no-scripts \
    --no-chown \
    --cache-dir "${CACHE_DIR}/apk" \
    --repository "${ALPINE_MIRROR}/${ALPINE_BRANCH}/main" \
    --repository "${ALPINE_MIRROR}/${ALPINE_BRANCH}/community" \
    add ${root_packages}

# Sanity checking: key promised tools must exist in the target root.
echo "==> Verifying installed developer tools"
check_binary() {
    local name="$1" dir rel
    for dir in bin usr/bin sbin usr/sbin; do
        rel="${dir}/${name}"
        if [ -e "${TARGET_ROOT}/${rel}" ] || [ -L "${TARGET_ROOT}/${rel}" ]; then
            echo "    ${rel}"
            return 0
        fi
    done
    echo "ERROR: missing installed binary: ${name}" >&2
    exit 1
}

for binary in bash gawk grep sed btop fastfetch htop gcc g++ make cmake \
    gdb git meson ninja pkg-config mkfs.ext4 mkfs.exfat openssl; do
    check_binary "${binary}"
done

if [ ! -e "${TARGET_ROOT}/lib/ld-musl-${ALPINE_ARCH}.so.1" ]; then
    echo "ERROR: missing musl loader lib/ld-musl-${ALPINE_ARCH}.so.1" >&2
    exit 1
fi

echo "    developer-tools successfully staged in ${TARGET_ROOT} ($(du -sh "${TARGET_ROOT}" | cut -f1))"
