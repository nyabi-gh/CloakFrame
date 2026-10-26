#!/usr/bin/env bash
# Plan M6 stage 5: opens the app's window on a virtual X display, which has no GPU, and saves a
# screenshot of it. Once as the app starts there, and once with wgpu unable to find an adapter, so
# the app falls back to OpenGL (Mesa's llvmpipe).
#
# Usage: app_window.sh <cloakframe executable> <output folder>
# Needs Xvfb, xdotool, ImageMagick, the X libraries winit opens and Mesa's Vulkan and OpenGL
# drivers.
set -euo pipefail

app=$(realpath "$1")
out=$2
mkdir -p "$out"
out=$(realpath "$out")

export DISPLAY=:99
Xvfb "$DISPLAY" -screen 0 1280x800x24 -nolisten tcp 2>"$out/xvfb.log" &
xvfb=$!
pid=
trap 'kill $pid $xvfb 2>/dev/null || true' EXIT
for _ in $(seq 100); do
    xdpyinfo >/dev/null 2>&1 && break
    sleep 0.1
done
xdpyinfo >/dev/null

# A blank screen has one colour; once the window has drawn its first frame it has many.
drawn() {
    local deviation
    deviation=$(convert "$1" -format '%[fx:standard_deviation]' info:)
    awk -v d="$deviation" 'BEGIN { exit !(d > 0.02) }'
}

# open <name> <the graphics line the log must show> [VAR=value...]
open() {
    local name=$1 expected=$2
    shift 2
    local log="$out/$name.log" shot="$out/$name.png"
    env XDG_DATA_HOME="$out/$name-data" "$@" "$app" >"$log" 2>&1 &
    pid=$!
    for _ in $(seq 600); do
        grep -q '^graphics: ' "$log" && break
        if ! kill -0 "$pid" 2>/dev/null; then
            echo "$name: the app ended before it drew" >&2
            cat "$log" >&2
            return 1
        fi
        sleep 0.1
    done
    timeout 60 xdotool search --sync --onlyvisible --name '^CloakFrame$' >/dev/null
    for _ in $(seq 300); do
        import -window root "$shot"
        drawn "$shot" && break
        sleep 0.1
    done
    drawn "$shot" || { echo "$name: the screen stayed blank" >&2; return 1; }
    kill "$pid"
    wait "$pid" || true
    pid=
    cat "$log"
    if ! grep -q "^graphics: $expected" "$log"; then
        echo "$name: expected graphics: $expected" >&2
        return 1
    fi
}

open default 'wgpu, '
# Point the Vulkan loader at no driver: wgpu then finds no adapter, as on a PC without one.
open opengl 'OpenGL, ' WGPU_BACKEND=vulkan VK_DRIVER_FILES=/nonexistent.json VK_ICD_FILENAMES=/nonexistent.json
grep -q '^wgpu could not start' "$out/opengl.log"
