#!/usr/bin/env bash
# Records the engine's first-pass detections on the video benchmark's clips in
# tests/fixtures/benchmark/video/detections, which CI tracks again (docs/M5_BENCHMARK.md). CPU,
# like the CI runner, with tile mode off (1.12.1's setting) and on (the default).
#
#   tools/benchmark/record_video.sh <onnxruntime library> <models dir> <ffmpeg> <ffprobe> <clips dir>
set -euo pipefail

repo=$(git rev-parse --show-toplevel)
manifest="$repo/tests/fixtures/benchmark/video/manifest.json"
out="$repo/tests/fixtures/benchmark/video/detections"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cargo build --release --locked -p cloakframe-engine --example video_benchmark
bench="$repo/target/release/examples/video_benchmark"

mkdir -p "$out"
for file in $(python3 -I -c 'import json,sys; [print(c["file"]) for c in json.load(open(sys.argv[1]))["clips"]]' "$manifest"); do
    name=${file%.*}
    for tiles in off on; do
        "$bench" --ort-library "$1" --models "$2" --ffmpeg "$3" --ffprobe "$4" --video "$5/$file" \
            --gpu off --tiles "$tiles" --out "$work/$name.json" --detections "$work/$name.jsonl"
        gzip -9n < "$work/$name.jsonl" > "$out/$name.tiles-$tiles.jsonl.gz"
    done
done
echo "detections written to $out"
