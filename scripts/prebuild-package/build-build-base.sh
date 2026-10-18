#!/bin/bash
# Stage Alpine's prebuilt C/C++ toolchain as an overlay layer (see apk-layer.sh).
set -euo pipefail
exec "$(dirname "$0")/apk-layer.sh" --name build-base --packages "build-base"
