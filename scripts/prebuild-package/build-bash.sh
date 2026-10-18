#!/bin/bash
# Stage the Alpine bash shell as an overlay layer (see apk-layer.sh).
set -euo pipefail
exec "$(dirname "$0")/apk-layer.sh" --name bash --packages "bash"
