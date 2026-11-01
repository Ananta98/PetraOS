#!/bin/bash
# ==============================================================================
# PetraOS amd64 Userland Builder (postmarketOS + OpenRC)
# ==============================================================================
# Assembles the PetraOS x86_64 userland using postmarketOS packages and Alpine
# minirootfs. Produces a lightweight in-memory initramfs (initramfs.cpio) and a
# full ext4 rootfs disk image (ext4.img) with postmarketos-base and OpenRC.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

ALPINE_VERSION="${ALPINE_VERSION:-3.20.3}"
ALPINE_BRANCH="${ALPINE_BRANCH:-v3.20}"
ALPINE_ARCH="x86_64"
ALPINE_SHA256="${ALPINE_SHA256:-d4e6fd67dcf75e40c451560ac7265166c2b72a0f38ddc9aae756a7de3d1efa0c}"
ALPINE_TAR_NAME="alpine-minirootfs-${ALPINE_VERSION}-${ALPINE_ARCH}.tar.gz"

PMOS_BRANCH="${PMOS_BRANCH:-v24.06}"
PMOS_MIRROR="${PMOS_MIRROR:-https://mirror.postmarketos.org/postmarketos}"
ALPINE_MIRROR="${ALPINE_MIRROR:-https://dl-cdn.alpinelinux.org/alpine}"
APK_TOOLS_VERSION="${APK_TOOLS_VERSION:-2.14.4-r1}"

CACHE_DIR="${REPO_ROOT}/build/cache"
BUILD_DIR="${REPO_ROOT}/build/userland-amd64"
STAGING="${BUILD_DIR}/staging"
ROOTFS_STAGING="${BUILD_DIR}/rootfs_staging"
INITRAMFS="${REPO_ROOT}/build/initramfs.cpio"
APK_STATIC="${CACHE_DIR}/apk.static"

PETRAOS_VERSION="${PETRAOS_VERSION:-0.1.0}"
SUPPORT_DIR="${REPO_ROOT}/scripts/build-support"

# postmarketOS rootfs packages
PMOS_PACKAGES=(
    postmarketos-base
    bash
    python3
    btop
    e2fsprogs
    exfatprogs
)
packages="${PETRAOS_PACKAGES:-${PMOS_PACKAGES[*]}}"

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

bootstrap_apk_static() {
    if [ -x "${APK_STATIC}" ]; then
        return
    fi

    local archive tmp
    archive="${CACHE_DIR}/apk-tools-static-${APK_TOOLS_VERSION}.apk"
    tmp="$(mktemp "${CACHE_DIR}/.apk.static.XXXXXX")"
    trap 'rm -f "${tmp}"' RETURN

    echo "==> Fetching apk-tools-static ${APK_TOOLS_VERSION}..."
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

if [ ! -f "${CACHE_DIR}/${ALPINE_TAR_NAME}" ]; then
    echo "==> Fetching Alpine ${ALPINE_VERSION} ${ALPINE_ARCH} minirootfs..."
    curl -fL --retry 3 \
        "${ALPINE_MIRROR}/${ALPINE_BRANCH}/releases/${ALPINE_ARCH}/${ALPINE_TAR_NAME}" \
        -o "${CACHE_DIR}/${ALPINE_TAR_NAME}"
fi

actual_sha256="$(sha256_file "${CACHE_DIR}/${ALPINE_TAR_NAME}")"
if [ "${actual_sha256}" != "${ALPINE_SHA256}" ]; then
    echo "ERROR: Alpine minirootfs checksum mismatch." >&2
    echo "Expected: ${ALPINE_SHA256}" >&2
    echo "Actual:   ${actual_sha256}" >&2
    exit 1
fi

echo "==> Extracting base minirootfs into initramfs staging (${STAGING})"
rm -rf "${STAGING}" "${ROOTFS_STAGING}"
mkdir -p "${STAGING}" "${ROOTFS_STAGING}"
tar -xzf "${CACHE_DIR}/${ALPINE_TAR_NAME}" -C "${STAGING}"

# Seed rootfs_staging with base files
cp -a "${STAGING}/." "${ROOTFS_STAGING}/"

echo "==> Installing postmarketOS packages into rootfs (${ROOTFS_STAGING})"
mkdir -p "${ROOTFS_STAGING}/etc/apk"
cat > "${ROOTFS_STAGING}/etc/apk/repositories" << EOF
${PMOS_MIRROR}/${PMOS_BRANCH}
${ALPINE_MIRROR}/${ALPINE_BRANCH}/main
${ALPINE_MIRROR}/${ALPINE_BRANCH}/community
EOF

"${APK_STATIC}" \
    --root "${ROOTFS_STAGING}" \
    --initdb \
    --arch "${ALPINE_ARCH}" \
    --allow-untrusted \
    --no-scripts \
    --no-chown \
    --cache-dir "${CACHE_DIR}/apk" \
    --repository "${PMOS_MIRROR}/${PMOS_BRANCH}" \
    --repository "${ALPINE_MIRROR}/${ALPINE_BRANCH}/main" \
    --repository "${ALPINE_MIRROR}/${ALPINE_BRANCH}/community" \
    add ${packages}

echo "==> Configuring the userlands (initramfs and rootfs)"
for target_dir in "${STAGING}" "${ROOTFS_STAGING}"; do
    mkdir -p "${target_dir}"/{bin,sbin,lib,usr/bin,usr/sbin,usr/lib,usr/share}
    mkdir -p "${target_dir}"/{dev/pts,dev/shm,proc,sys,tmp,run,root,mnt,media,var/log,var/run,var/tmp,sysroot}
    mkdir -p "${target_dir}/root/.config/btop"
    chmod 1777 "${target_dir}/tmp" "${target_dir}/var/tmp"
    chmod 0750 "${target_dir}/root"
    install -m0755 "${SUPPORT_DIR}/init" "${target_dir}/init"

    # Ensure /bin/bash fallback
    if [ ! -e "${target_dir}/bin/bash" ]; then
        ln -sf sh "${target_dir}/bin/bash"
    fi

    # Ensure /usr/bin/python symlink
    if [ -x "${target_dir}/usr/bin/python3" ] && [ ! -e "${target_dir}/usr/bin/python" ]; then
        ln -sfn python3 "${target_dir}/usr/bin/python"
    fi
done

for target_dir in "${STAGING}" "${ROOTFS_STAGING}"; do
    cat > "${target_dir}/etc/passwd" << 'EOF'
root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
daemon:x:2:2:daemon:/sbin:/sbin/nologin
nobody:x:65534:65534:nobody:/:/sbin/nologin
EOF

    cat > "${target_dir}/etc/shells" << 'EOF'
/bin/sh
/bin/bash
EOF

    cat > "${target_dir}/etc/group" << 'EOF'
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

    cat > "${target_dir}/etc/hosts" << 'EOF'
127.0.0.1 localhost petraos
::1       localhost ip6-localhost ip6-loopback
EOF

    cat > "${target_dir}/etc/resolv.conf" << 'EOF'
nameserver 10.0.2.3
nameserver 1.1.1.1
nameserver 8.8.8.8
EOF

    cat > "${target_dir}/etc/hostname" << 'EOF'
petraos
EOF

    cat > "${target_dir}/etc/environment" << 'EOF'
CHARSET=UTF-8
LANG=C.UTF-8
LC_ALL=C.UTF-8
EOF

    printf '%s\n' "${PETRAOS_VERSION}" > "${target_dir}/etc/petraos-release"

    # In STAGING, provide os-release if missing
    if [ ! -f "${target_dir}/etc/os-release" ] && [ ! -f "${target_dir}/usr/lib/os-release" ]; then
        mkdir -p "${target_dir}/usr/lib"
        cat > "${target_dir}/usr/lib/os-release" << EOF
NAME="postmarketOS"
PRETTY_NAME="postmarketOS (PetraOS Kernel)"
ID="postmarketos"
ID_LIKE="alpine"
VERSION_ID="v24.06"
VERSION="v24.06"
EOF
        ln -sf ../usr/lib/os-release "${target_dir}/etc/os-release"
    fi

    cat > "${target_dir}/etc/profile" << 'EOF'
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
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
alias btop='btop -t'
umask 022

for script in /etc/profile.d/*.sh ; do
    if [ -r "$script" ] ; then
        . "$script"
    fi
done
unset script
EOF

    cat > "${target_dir}/root/.profile" << 'EOF'
export PS1='petraos:\w# '
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export SHELL=/bin/bash
export TERM=linux
export HOME=/root
export XDG_CONFIG_HOME=/root/.config
export CHARSET=UTF-8
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
alias ll='ls -la --color=auto'
alias l='ls -lh'
alias btop='btop -t'
EOF

    cat > "${target_dir}/root/.config/btop/btop.conf" << 'EOF'
force_tty = True
graph_symbol = "tty"
rounded_corners = False
EOF

    # Best-effort device nodes; skipped when the build runs unprivileged.
    mknod -m 666 "${target_dir}/dev/null" c 1 3 2>/dev/null || true
    mknod -m 666 "${target_dir}/dev/zero" c 1 5 2>/dev/null || true
    mknod -m 666 "${target_dir}/dev/tty" c 5 0 2>/dev/null || true
    mknod -m 600 "${target_dir}/dev/console" c 5 1 2>/dev/null || true
done

for path in "${STAGING}/bin/busybox" "${STAGING}/lib/ld-musl-${ALPINE_ARCH}.so.1"; do
    if [ ! -e "$path" ]; then
        echo "ERROR: Initramfs staging is incomplete, missing ${path}" >&2
        exit 1
    fi
done

echo "==> Packaging the initramfs and disk images"
"${SCRIPT_DIR}/../build-initramfs.sh" --staging "${STAGING}" --rootfs-staging "${ROOTFS_STAGING}" --output "${INITRAMFS}"

file "${STAGING}/bin/busybox"
