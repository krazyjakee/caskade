#!/usr/bin/env bash
# Chromium's renderings of tests/text_pixels.rs's pages (#80): the test
# writes the pages it renders with Blitz, headless Chromium screenshots
# each on a private desktop, and the screenshots become tests/text/<page>.png,
# which the test measures beside Blitz's. Run after changing a page.
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
[ -f "$out/chromium.html" ] || { echo "the test didn't write its pages" >&2; exit 1; }

chrome=${CHROME:-$(ls -d "$HOME"/.cache/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-linux64/chrome-headless-shell 2>/dev/null | sort | tail -1)}
[ -x "$chrome" ] || { echo "no headless Chromium: set CHROME" >&2; exit 1; }
for page in "$out"/*.html; do
  name=$(basename "$page" .html)
  "$repo/scripts/private-desktop.sh" -- "$chrome" --headless --no-sandbox --disable-gpu \
    --hide-scrollbars --force-device-scale-factor=1 --disable-lcd-text \
    --font-render-hinting=none --window-size=400,260 \
    --screenshot="$native/tests/text/$name.png" "file://$page" 2>/dev/null
  echo "wrote $native/tests/text/$name.png"
done
echo "with $("$chrome" --version 2>/dev/null || echo Chromium)"
