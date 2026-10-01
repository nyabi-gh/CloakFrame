#!/usr/bin/env bash
set -euo pipefail

# Checks that each bundled FFmpeg tool still matches the checksum manifest next to it.
# CloakFrame refuses a bundled tool whose manifest is missing or differs, so this has to run
# after the last step that changes the binaries, such as code signing.
#
# Usage: check_ffmpeg_manifests.sh <directory>

if [[ $# -ne 1 ]]; then
    echo "usage: check_ffmpeg_manifests.sh <directory>" >&2
    exit 2
fi

sha256() {
    if command -v sha256sum >/dev/null; then
        sha256sum <"$1" | cut -d' ' -f1
    else
        shasum -a 256 <"$1" | cut -d' ' -f1
    fi
}

# Git Bash reports `ffmpeg` as present when only `ffmpeg.exe` is, so pick the name first.
suffix=""
[[ -f "$1/ffmpeg.exe" ]] && suffix=".exe"
for tool in ffmpeg ffprobe; do
    binary="$1/$tool$suffix"
    if [[ ! -f "$binary" ]]; then
        echo "$binary is missing." >&2
        exit 1
    fi
    if [[ ! -f "$binary.sha256" ]]; then
        echo "$binary has no checksum manifest." >&2
        exit 1
    fi
    expected="$(cut -d' ' -f1 "$binary.sha256" | tr -d '\r\n' | tr '[:upper:]' '[:lower:]')"
    actual="$(sha256 "$binary")"
    if [[ "$expected" != "$actual" ]]; then
        echo "$binary does not match its manifest ($actual, expected $expected)." >&2
        exit 1
    fi
    echo "$binary: matches its manifest"
done
