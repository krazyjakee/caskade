#!/usr/bin/env bash
# Chromium's rendering of tests/text_pixels.rs's page (#80): the test
# writes the page it renders with Blitz, headless Chromium screenshots it
# on a private desktop, and the screenshot becomes tests/text/chromium.png,
# which the test measures beside Blitz's. Run after changing the page.
#
#   apps/native/scripts/text-oracle.sh
#
# CHROME names the browser; by default, Playwright's headless shell.
set -euo pipefail
native=$(cd "$(dirname "$0")/.." && pwd)
repo=$(cd "$native/../.." && pwd)
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

# The page (the test fails without the screenshot; the page is written first).
CASKADE_TEXT_PAGE="$out" flock /tmp/caskade-heavy.lock \
  cargo test -p caskade-native --features shots --test text_pixels >/dev/null 2>&1 || true
[ -f "$out/page.html" ] || { echo "the test didn't write its page" >&2; exit 1; }

chrome=${CHROME:-$(ls -d "$HOME"/.cache/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-linux64/chrome-headless-shell 2>/dev/null | sort | tail -1)}
[ -x "$chrome" ] || { echo "no headless Chromium: set CHROME" >&2; exit 1; }
"$repo/scripts/private-desktop.sh" -- "$chrome" --headless --no-sandbox --disable-gpu \
  --hide-scrollbars --force-device-scale-factor=1 --disable-lcd-text \
  --font-render-hinting=none --window-size=400,260 \
  --screenshot="$native/tests/text/chromium.png" "file://$out/page.html" 2>/dev/null
echo "wrote $native/tests/text/chromium.png ($("$chrome" --version 2>/dev/null || echo Chromium))"
