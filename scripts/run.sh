#!/usr/bin/env bash
set -euo pipefail

# Run the desktop app with hot reload from any working directory.
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

exec pnpm tauri dev "$@"
