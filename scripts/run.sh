#!/usr/bin/env bash
set -euo pipefail

# Build and run the app from any working directory.
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

exec just dev "$@"
