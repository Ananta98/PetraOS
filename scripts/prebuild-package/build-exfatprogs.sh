#!/bin/bash
# Stage Alpine exfatprogs so the guest can create and check exFAT filesystems.
set -euo pipefail
exec "$(dirname "$0")/apk-layer.sh" --name exfatprogs --packages "exfatprogs"
