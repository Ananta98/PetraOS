#!/bin/bash
# Stage the Alpine fastfetch system information tool as an overlay layer.
set -euo pipefail
exec "$(dirname "$0")/apk-layer.sh" --name fastfetch --packages "fastfetch"
