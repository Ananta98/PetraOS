#!/bin/bash
# Stage the Alpine btop resource monitor as an overlay layer (see apk-layer.sh).
set -euo pipefail
exec "$(dirname "$0")/apk-layer.sh" --name btop --packages "btop"
