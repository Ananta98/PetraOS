#!/bin/bash
# Assemble the PetraOS amd64 userland from an official Alpine minirootfs and
# prebuilt package layers, then hand the staging root to the initramfs packer.
#
# Usage: build-userland-amd64.sh [--fresh] [--purge-cache]
#
#   --fresh        remove existing userland artifacts before rebuilding
#   --purge-cache  with --fresh, also drop the download cache and re-fetch Alpine
#
# Environment:
#   PETRAOS_USERLAND_PACKAGES  space separated prebuild-package layer names
#                              (default: developer-tools)
#   PETRAOS_VERSION            version string reported by /etc/os-release
#   ALPINE_VERSION/ALPINE_BRANCH/ALPINE_SHA256  override the Alpine release
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

ALPINE_VERSION="${ALPINE_VERSION:-3.20.3}"
ALPINE_BRANCH="${ALPINE_BRANCH:-v3.20}"
ALPINE_ARCH="x86_64"
ALPINE_SHA256="${ALPINE_SHA256:-d4e6fd67dcf75e40c451560ac7265166c2b72a0f38ddc9aae756a7de3d1efa0c}"
ALPINE_TAR_NAME="alpine-minirootfs-${ALPINE_VERSION}-${ALPINE_ARCH}.tar.gz"

CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/userland-amd64"
STAGING="${BUILD_DIR}/staging"
INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"

PACKAGES="${PETRAOS_USERLAND_PACKAGES-developer-tools}"
PETRAOS_VERSION="${PETRAOS_VERSION:-0.1.0}"
SUPPORT_DIR="${REPO_ROOT}/scripts/build-support"
PREBUILD_DIR="${REPO_ROOT}/scripts/prebuild-package"

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

for tool in curl tar file; do
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

# Replace symlinked entries before copying so that a package file never writes
# through a BusyBox applet link and clobbers /bin/busybox itself.
merge_layer() {
    local layer="$1" source relative destination
    [ -d "$layer" ] || return 0
    while IFS= read -r -d '' source; do
        relative="${source#"$layer/"}"
        destination="${STAGING}/${relative}"
        if [ -L "$destination" ]; then
            rm -f "$destination"
        fi
    done < <(find "$layer" -mindepth 1 -print0)
    cp -a "$layer/." "${STAGING}/"
}

mkdir -p "${CACHE_DIR}" "${BUILD_DIR}"

if [ ! -f "${CACHE_DIR}/${ALPINE_TAR_NAME}" ]; then
    echo "==> Fetching Alpine ${ALPINE_VERSION} ${ALPINE_ARCH} minirootfs..."
    curl -fL --retry 3 \
        "https://dl-cdn.alpinelinux.org/alpine/${ALPINE_BRANCH}/releases/${ALPINE_ARCH}/${ALPINE_TAR_NAME}" \
        -o "${CACHE_DIR}/${ALPINE_TAR_NAME}"
fi

actual_sha256="$(sha256_file "${CACHE_DIR}/${ALPINE_TAR_NAME}")"
if [ "${actual_sha256}" != "${ALPINE_SHA256}" ]; then
    echo "ERROR: Alpine minirootfs checksum mismatch." >&2
    echo "Expected: ${ALPINE_SHA256}" >&2
    echo "Actual:   ${actual_sha256}" >&2
    exit 1
fi

echo "==> Extracting the Alpine userland into ${STAGING}"
rm -rf "${STAGING}"
mkdir -p "${STAGING}"
tar -xzf "${CACHE_DIR}/${ALPINE_TAR_NAME}" -C "${STAGING}"

echo "==> Staging prebuilt package layers"
for package in ${PACKAGES}; do
    layer_script="${PREBUILD_DIR}/build-${package}.sh"
    if [ ! -x "${layer_script}" ]; then
        echo "ERROR: no prebuild-package script for '${package}': ${layer_script}" >&2
        exit 1
    fi
    echo "    ${package}"
    "${layer_script}"
    merge_layer "${REPO_ROOT}/build/prebuild-package/${package}/staging"
done

echo "==> Configuring the userland"
mkdir -p "${STAGING}"/{bin,sbin,lib,usr/bin,usr/sbin,usr/lib,usr/share}
mkdir -p "${STAGING}"/{dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,media,var/log,var/run,var/tmp}
mkdir -p "${STAGING}/etc/fastfetch" "${STAGING}/root/.config/btop" \
    "${STAGING}/root/.config/fastfetch"
chmod 1777 "${STAGING}/tmp" "${STAGING}/var/tmp"
chmod 0750 "${STAGING}/root"

install -m0755 "${SUPPORT_DIR}/init-amd64/init" "${STAGING}/init"

cat > "${STAGING}/etc/passwd" << 'EOF'
root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
daemon:x:2:2:daemon:/sbin:/sbin/nologin
nobody:x:65534:65534:nobody:/:/sbin/nologin
EOF

cat > "${STAGING}/etc/shells" << 'EOF'
/bin/sh
/bin/bash
EOF

cat > "${STAGING}/etc/group" << 'EOF'
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

cat > "${STAGING}/etc/hosts" << 'EOF'
127.0.0.1 localhost petraos
::1       localhost ip6-localhost ip6-loopback
EOF

cat > "${STAGING}/etc/resolv.conf" << 'EOF'
nameserver 10.0.2.3
nameserver 1.1.1.1
nameserver 8.8.8.8
EOF

cat > "${STAGING}/etc/hostname" << 'EOF'
petraos
EOF

cat > "${STAGING}/etc/environment" << 'EOF'
CHARSET=UTF-8
LANG=C.UTF-8
LC_ALL=C.UTF-8
EOF

printf '%s\n' "${PETRAOS_VERSION}" > "${STAGING}/etc/petraos-release"

mkdir -p "${STAGING}/usr/lib"
cat > "${STAGING}/usr/lib/os-release" << EOF
NAME="PetraOS"
PRETTY_NAME="PetraOS Silica"
ID=petraos
ID_LIKE=alpine
VERSION_ID="${PETRAOS_VERSION}"
VERSION="${PETRAOS_VERSION}"
EOF
ln -sf ../usr/lib/os-release "${STAGING}/etc/os-release"

cat > "${STAGING}/etc/profile" << 'EOF'
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
export HOME=/root
export SHELL=/bin/bash
export TERM=linux
export XDG_CONFIG_HOME=/root/.config
export CHARSET=UTF-8
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export PS1='petraos:\w# '
alias ll='ls -la --color=auto'
alias l='ls -lh'
alias fastfetch='fastfetch --config /root/.config/fastfetch/config.jsonc'
alias btop='btop -t'
umask 022

for script in /etc/profile.d/*.sh ; do
    if [ -r "$script" ] ; then
        . "$script"
    fi
done
unset script
EOF

cat > "${STAGING}/root/.profile" << 'EOF'
export PS1='petraos:\w# '
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
export SHELL=/bin/bash
export TERM=linux
export HOME=/root
export XDG_CONFIG_HOME=/root/.config
export CHARSET=UTF-8
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
alias ll='ls -la --color=auto'
alias l='ls -lh'
alias fastfetch='fastfetch --config /root/.config/fastfetch/config.jsonc'
alias btop='btop -t'
EOF

cat > "${STAGING}/root/.config/btop/btop.conf" << 'EOF'
force_tty = True
graph_symbol = "tty"
rounded_corners = False
EOF

install -m0644 "${SUPPORT_DIR}/petraos-logo.txt" "${STAGING}/etc/fastfetch/petraos.txt"
install -m0644 "${SUPPORT_DIR}/fastfetch.jsonc" "${STAGING}/root/.config/fastfetch/config.jsonc"

# Best-effort device nodes; skipped when the build runs unprivileged.
mknod -m 666 "${STAGING}/dev/null" c 1 3 2>/dev/null || true
mknod -m 666 "${STAGING}/dev/zero" c 1 5 2>/dev/null || true
mknod -m 666 "${STAGING}/dev/tty" c 5 0 2>/dev/null || true
mknod -m 600 "${STAGING}/dev/console" c 5 1 2>/dev/null || true

for path in "${STAGING}/bin/busybox" "${STAGING}/lib/ld-musl-${ALPINE_ARCH}.so.1"; do
    if [ ! -e "$path" ]; then
        echo "ERROR: Alpine userland is incomplete, missing ${path}" >&2
        exit 1
    fi
done

echo "==> Packaging the initramfs and disk images"
"${SCRIPT_DIR}/../build-initramfs.sh" --staging "${STAGING}" --output "${INITRAMFS}"

file "${STAGING}/bin/busybox"
