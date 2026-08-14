#!/usr/bin/env bash
# ==============================================================================
# PetraOS Initramfs Creation Tool
#
# Packages a directory tree into a standard SVR4 portable format (newc) CPIO archive.
# ==============================================================================

set -euo pipefail

ROOT_DIR="${1:-initramfs_root}"
OUTPUT_CPIO="${2:-initramfs.cpio}"

# Ensure source root directory exists
if [ ! -d "${ROOT_DIR}" ]; then
    echo "[INFO] Creating directory tree '${ROOT_DIR}'..."
    mkdir -p "${ROOT_DIR}/sbin" "${ROOT_DIR}/bin" "${ROOT_DIR}/etc"
fi

echo "[INFO] Packaging '${ROOT_DIR}' into '${OUTPUT_CPIO}'..."

if command -v cpio &>/dev/null; then
    # Use standard host cpio with SVR4 portable format (newc)
    (
        cd "${ROOT_DIR}"
        find . -mindepth 1 | cpio -o -H newc -R 0:0 > "../${OUTPUT_CPIO}"
    )
elif command -v python3 &>/dev/null; then
    # Fallback to Python-based SVR4 newc packer if host lacks cpio
    python3 -c "
import os, sys

root = '${ROOT_DIR}'
out_file = open('${OUTPUT_CPIO}', 'wb')
ino = 1

def pad(n, m=4):
    return (m - (n % m)) % m

for dirpath, dirnames, filenames in os.walk(root):
    for name in dirnames + filenames:
        full = os.path.join(dirpath, name)
        rel = os.path.relpath(full, root).replace('\\\\', '/')
        st = os.lstat(full)
        is_dir = os.path.isdir(full)
        mode = 0o040755 if is_dir else 0o100755
        size = 0 if is_dir else st.st_size
        namesize = len(rel.encode('utf-8')) + 1
        
        # SVR4 newc header (110 bytes)
        hdr = f'070701{ino:08x}{mode:08x}{0:08x}{0:08x}{1:08x}{int(st.st_mtime):08x}{size:08x}{0:08x}{0:08x}{0:08x}{0:08x}{namesize:08x}{0:08x}'
        out_file.write(hdr.encode('ascii'))
        out_file.write(rel.encode('utf-8') + b'\\x00')
        out_file.write(b'\\x00' * pad(110 + namesize))
        
        if not is_dir and size > 0:
            with open(full, 'rb') as f:
                out_file.write(f.read())
            out_file.write(b'\\x00' * pad(size))
        ino += 1

# Trailer
trailer = 'TRAILER!!!'
trailer_namesize = len(trailer.encode('utf-8')) + 1
hdr = f'070701{0:08x}{0:08x}{0:08x}{0:08x}{1:08x}{0:08x}{0:08x}{0:08x}{0:08x}{0:08x}{0:08x}{trailer_namesize:08x}{0:08x}'
out_file.write(hdr.encode('ascii'))
out_file.write(trailer.encode('utf-8') + b'\\x00')
out_file.write(b'\\x00' * pad(110 + trailer_namesize))
out_file.close()
"
else
    echo "[ERROR] Neither 'cpio' nor 'python3' was found to build the initramfs archive." >&2
    exit 1
fi

echo "✔ [INFO] Generated ${OUTPUT_CPIO} ($(wc -c < "${OUTPUT_CPIO}") bytes)"
