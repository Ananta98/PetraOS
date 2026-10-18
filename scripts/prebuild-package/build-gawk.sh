#!/bin/bash
# Stage GNU awk as an overlay layer (see apk-layer.sh).
set -euo pipefail
exec "$(dirname "$0")/apk-layer.sh" --name gawk --packages "gawk"
