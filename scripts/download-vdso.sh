#!/bin/bash
# Download prebuilt Linux vDSO library from asterinas/linux_vdso
# Repository: https://github.com/asterinas/linux_vdso
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
TARGET_DIR="${REPO_ROOT}/petra/src/arch/x86_64/vdso"

VDSO_URL="https://raw.githubusercontent.com/asterinas/linux_vdso/main/vdso_x86_64.so"

echo "==> Downloading Linux vDSO x86_64 from ${VDSO_URL}"
curl -sSL -o "${TARGET_DIR}/vdso_x86_64.so" "${VDSO_URL}"
echo "==> Successfully downloaded vdso_x86_64.so ($(stat -c%s "${TARGET_DIR}/vdso_x86_64.so") bytes)"
