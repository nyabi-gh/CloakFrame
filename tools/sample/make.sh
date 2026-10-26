#!/bin/sh
# Makes crates/cloakframe-app/assets/sample/astronaut.webp, the main screen's sample photo, from
# the test assets' astronaut.png (NASA, public domain; scikit-image v0.25.2 data).
# Usage: tools/sample/make.sh <test assets dir>   (python3 -I tools/fetch_test_assets.py fetches it)
# Needs ImageMagick and cwebp.
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
magick "$1/astronaut.png" -resize 256x256 -strip "$work/astronaut.png"
cwebp -quiet -q 82 -metadata none "$work/astronaut.png" -o "$root/crates/cloakframe-app/assets/sample/astronaut.webp"
