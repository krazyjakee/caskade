#!/usr/bin/env bash
# Downloads the website screenshots' cover art (#150): Steam's public
# library covers of the sample games (WEBSITE_GAMES in
# apps/native/src/frame/shot.rs), as <folder>/<Steam app id>.jpg. A manual
# step, run by `just website-shots`: the render itself and every test stay
# offline. Covers already in the folder are kept.
set -euo pipefail

folder="${1:?usage: scripts/website-art.sh <folder>}"
mkdir -p "$folder"
for id in 1145360 367520 413150 504230 620 105600 588650 275850; do
    if [ -s "$folder/$id.jpg" ]; then
        continue
    fi
    curl --fail --silent --show-error --location --max-time 30 \
        --output "$folder/$id.jpg.part" \
        "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/$id/library_600x900.jpg"
    mv "$folder/$id.jpg.part" "$folder/$id.jpg"
done
