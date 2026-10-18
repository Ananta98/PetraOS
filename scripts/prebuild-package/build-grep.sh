#!/bin/bash
# Stage GNU grep as an overlay layer (see apk-layer.sh).
set -euo pipefail
exec "$(dirname "$0")/apk-layer.sh" --name grep --packages "grep"
