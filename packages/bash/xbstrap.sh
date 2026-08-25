#!/usr/bin/env bash
# ==============================================================================
# PetraOS xbstrap Package Wrapper
# ==============================================================================

set -euo pipefail

PKG_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKG_NAME="$(basename "${PKG_DIR}")"
ROOT_DIR="$(cd "${PKG_DIR}/../.." && pwd)"

exec "${ROOT_DIR}/tools/xbstrap.sh" "${1:-build}" "${PKG_NAME}" "${@:2}"
