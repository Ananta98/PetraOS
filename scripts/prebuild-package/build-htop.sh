#!/bin/bash
# Stage the Alpine htop process viewer as an overlay layer (see apk-layer.sh).
set -euo pipefail
exec "$(dirname "$0")/apk-layer.sh" --name htop --packages "htop"
