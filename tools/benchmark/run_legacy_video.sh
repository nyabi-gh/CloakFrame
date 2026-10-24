#!/usr/bin/env bash
# Records CloakFrame 1.12.1's tracks on the video benchmark's clips in
# tests/fixtures/benchmark/video/legacy (docs/M5_BENCHMARK.md).
#
# Needs macOS with the 1.12.1 Homebrew dependencies, like run_legacy.sh. Both use the pinned FFmpeg
# so the clips decode the same as in the engine.
#
#   tools/benchmark/run_legacy_video.sh <models dir> <ffmpeg> <ffprobe> <clips dir> [work dir]
#
# <clips dir> holds the clips of tests/fixtures/benchmark/video/manifest.json
# (tools/benchmark/video_fetch.py).
set -euo pipefail

repo=$(git rev-parse --show-toplevel)
models=$(cd "$1" && pwd)
ffmpeg=$2
ffprobe=$3
clips=$(cd "$4" && pwd)
work=${5:-$(mktemp -d)}
mkdir -p "$work"
work=$(cd "$work" && pwd)
out="$repo/tests/fixtures/benchmark/video/legacy"

rm -rf "$work/src" "$work/build"
mkdir -p "$work/src"
git -C "$repo" archive v1.12.1 | tar -x -C "$work/src"
echo "add_subdirectory(\"$repo/tools/benchmark/legacy\" benchmark-legacy)" >> "$work/src/CMakeLists.txt"
cmake -S "$work/src" -B "$work/build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF \
    -DCLOAKFRAME_SELF_UPDATE=OFF > "$work/configure.log"
cmake --build "$work/build" --target cloakframe_bench_legacy_video > "$work/build.log"
bench="$work/build/benchmark-legacy/cloakframe_bench_legacy_video"

mkdir -p "$out"
for file in $(python3 -I -c 'import json,sys; [print(c["file"]) for c in json.load(open(sys.argv[1]))["clips"]]' \
        "$repo/tests/fixtures/benchmark/video/manifest.json"); do
    name=${file%.*}
    "$bench" "$models" "$ffmpeg" "$ffprobe" "$clips/$file" "$work/$name.json" yolo5face off 0.50 0.4
    gzip -9n < "$work/$name.json" > "$out/$name.legacy.json.gz"
done
echo "1.12.1 tracks written to $out"
