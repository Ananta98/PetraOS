#!/bin/bash
# Stage one or more prebuilt Alpine packages as a reusable filesystem layer.
#
# The layer is a plain directory tree (the package payload) that the userland
# builder overlays onto its staging root. Packages are resolved and extracted
# with Alpine's own `apk.static`, so dependency closure and file layout match
# a real Alpine install without needing root or a chroot.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

ALPINE_BRANCH="${ALPINE_BRANCH:-v3.20}"
ALPINE_ARCH="${ALPINE_ARCH:-x86_64}"
ALPINE_MIRROR="${ALPINE_MIRROR:-https://dl-cdn.alpinelinux.org/alpine}"
APK_TOOLS_VERSION="${APK_TOOLS_VERSION:-2.14.4-r1}"

CACHE_DIR="${REPO_ROOT}/build/cache"
APK_STATIC="${CACHE_DIR}/apk.static"

LAYER_NAME=""
PACKAGES=""
OUTPUT=""

usage() {
    echo "Usage: $0 --name <layer> --packages \"<pkg> [<pkg>...]\" [--output <dir>]"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --name)
            LAYER_NAME="$2"
            shift 2
            ;;
        --packages)
            PACKAGES="$2"
            shift 2
            ;;
        --output)
            OUTPUT="$2"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "ERROR: unknown option: $1" >&2
            usage
            ;;
    esac
done

if [ -z "${LAYER_NAME}" ] || [ -z "${PACKAGES}" ]; then
    echo "ERROR: --name and --packages are required" >&2
    usage
fi

OUTPUT="${OUTPUT:-${REPO_ROOT}/build/prebuild-package/${LAYER_NAME}/staging}"

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

    echo "    fetching apk-tools-static ${APK_TOOLS_VERSION}"
    if [ ! -s "${archive}" ]; then
        curl -fL --retry 3 \
            "${ALPINE_MIRROR}/${ALPINE_BRANCH}/main/${ALPINE_ARCH}/apk-tools-static-${APK_TOOLS_VERSION}.apk" \
            -o "${archive}"
    fi
    tar xOf "${archive}" sbin/apk.static > "${tmp}"
    chmod 0755 "${tmp}"
    mv -f "${tmp}" "${APK_STATIC}"
}

mkdir -p "${CACHE_DIR}"
bootstrap_apk_static

echo "==> Staging Alpine layer '${LAYER_NAME}' (${PACKAGES})"
rm -rf "${OUTPUT}"
mkdir -p "${OUTPUT}"

# --no-scripts keeps post-install hooks from chrooting, --no-chown lets this
# run as an unprivileged user, and --allow-untrusted skips key verification.
"${APK_STATIC}" \
    --root "${OUTPUT}" \
    --initdb \
    --arch "${ALPINE_ARCH}" \
    --allow-untrusted \
    --no-scripts \
    --no-chown \
    --cache-dir "${CACHE_DIR}/apk" \
    --repository "${ALPINE_MIRROR}/${ALPINE_BRANCH}/main" \
    --repository "${ALPINE_MIRROR}/${ALPINE_BRANCH}/community" \
    add ${PACKAGES}

# Drop the layer's package database and world file so that merging many layers
# never clobbers the base rootfs's apk database.
rm -rf "${OUTPUT}/lib/apk/db" "${OUTPUT}/etc/apk/world"

echo "    layer staged at ${OUTPUT}"
