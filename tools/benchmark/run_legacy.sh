#!/usr/bin/env bash
# Records CloakFrame 1.12.1's detections on the benchmark images in tests/fixtures/benchmark/legacy.
#
# Needs macOS with the 1.12.1 Homebrew dependencies. The legacy sources are exported to a work
# directory and built there. On Apple silicon Homebrew's OpenCV resizes through KleidiCV, one level
# off x86 OpenCV, which the engine follows; a few boxes differ slightly (docs/M3_BENCHMARK.md).
#
#   tools/benchmark/run_legacy.sh <models dir> <images dir> [work dir]
#
# <images dir> holds the images of tests/fixtures/benchmark/manifest.json (tools/benchmark/fetch.py).
set -euo pipefail

repo=$(git rev-parse --show-toplevel)
models=$(cd "$1" && pwd)
images=$(cd "$2" && pwd)
work=${3:-$(mktemp -d)}
mkdir -p "$work"
work=$(cd "$work" && pwd)
manifest="$repo/tests/fixtures/benchmark/manifest.json"
out="$repo/tests/fixtures/benchmark/legacy"

rm -rf "$work/src" "$work/build"
mkdir -p "$work/src"
git -C "$repo" archive v1.12.1 | tar -x -C "$work/src"
echo "add_subdirectory(\"$repo/tools/benchmark/legacy\" benchmark-legacy)" >> "$work/src/CMakeLists.txt"
cmake -S "$work/src" -B "$work/build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF \
    -DCLOAKFRAME_SELF_UPDATE=OFF > "$work/configure.log"
cmake --build "$work/build" --target cloakframe_bench_legacy > "$work/build.log"
bench="$work/build/benchmark-legacy/cloakframe_bench_legacy"

mkdir -p "$out"
for score in 0.30 0.40 0.50; do
    "$bench" "$models" "$manifest" "$images" "$out/yolo5face-n@$score.json" yolo5face off "$score" 0.4
done
"$bench" "$models" "$manifest" "$images" "$out/yunet@0.50.json" yunet off 0.50 0.4
"$bench" "$models" "$manifest" "$images" "$out/plate@0.50.json" off on 0.50 0.4
echo "1.12.1 detections written to $out"
