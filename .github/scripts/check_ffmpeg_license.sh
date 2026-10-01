#!/usr/bin/env bash
set -euo pipefail

# Refuses an FFmpeg build CloakFrame may not ship. A build configured with --enable-nonfree
# links code whose license conflicts with the GPL, and FFmpeg marks the result as
# unredistributable. THIRD_PARTY_NOTICES.txt states GPL-3.0-or-later, so the build also has to
# be configured with --enable-gpl and --enable-version3.
#
# Usage: check_ffmpeg_license.sh <binary>...

if [[ $# -eq 0 ]]; then
    echo "usage: check_ffmpeg_license.sh <binary>..." >&2
    exit 2
fi

for binary in "$@"; do
    configuration="$("$binary" -hide_banner -buildconf)"
    if grep -q -- '--enable-nonfree' <<<"$configuration"; then
        echo "$binary was configured with --enable-nonfree and cannot be redistributed." >&2
        exit 1
    fi
    for required in --enable-gpl --enable-version3; do
        if ! grep -q -- "$required" <<<"$configuration"; then
            echo "$binary was not configured with $required, so THIRD_PARTY_NOTICES.txt" \
                "misstates its license." >&2
            exit 1
        fi
    done
    echo "$binary: GPL-3.0-or-later, no nonfree components"
done
