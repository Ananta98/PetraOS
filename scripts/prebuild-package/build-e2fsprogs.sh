#!/bin/bash
# Stage Alpine e2fsprogs so the guest can create and check ext2 filesystems.
set -euo pipefail
exec "$(dirname "$0")/apk-layer.sh" --name e2fsprogs --packages "e2fsprogs"
