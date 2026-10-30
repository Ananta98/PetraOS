#!/bin/bash
# Build the PetraOS linux-vdso.so.1 shared library for x86_64.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

OUT_DIR="${REPO_ROOT}/petra/src/arch/x86_64/vdso"
mkdir -p "${OUT_DIR}"

CFLAGS=(
    -O2
    -Wall
    -Wextra
    -Werror
    -fPIC
    -shared
    -nostdlib
    -fno-asynchronous-unwind-tables
    -fno-stack-protector
    -fomit-frame-pointer
    -m64
)

LDFLAGS=(
    -Wl,-soname=linux-vdso.so.1
    -Wl,--version-script="${SCRIPT_DIR}/vdso.version"
    -Wl,-T,"${SCRIPT_DIR}/vdso.lds"
    -Wl,-Bsymbolic
    -Wl,-z,max-page-size=4096
    -Wl,-z,noexecstack
    -Wl,--hash-style=both
    -Wl,--eh-frame-hdr
    -Wl,--build-id=none
)

gcc "${CFLAGS[@]}" \
    "${SCRIPT_DIR}/vdso.c" \
    "${SCRIPT_DIR}/sigreturn.S" \
    "${LDFLAGS[@]}" \
    -o "${OUT_DIR}/vdso.so"

strip --strip-unneeded "${OUT_DIR}/vdso.so"

echo "Successfully built ${OUT_DIR}/vdso.so ($(stat -c%s "${OUT_DIR}/vdso.so") bytes)"
