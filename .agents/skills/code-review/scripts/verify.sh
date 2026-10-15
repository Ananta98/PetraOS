#!/usr/bin/env bash
set -euo pipefail

# Ensure rust & cargo binaries are in PATH
export PATH="$HOME/.cargo/bin:$PATH"

echo "========================================"
echo " PetraOS Build & Correctness Gate"
echo "========================================"

echo "[1/2] Checking compilation (cargo osdk check)..."
cargo osdk check

echo "[2/2] Building kernel binary (cargo osdk build)..."
cargo osdk build

echo "========================================"
echo " All checks passed cleanly!"
echo "========================================"
