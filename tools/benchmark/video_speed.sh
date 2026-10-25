#!/usr/bin/env bash
# Times whole video runs of the engine's CLI and of CloakFrame 1.12.1 on the video benchmark's
# clips and a 4K copy of the street clip (plan 9.3, docs/M5_VERIFY.md). Each setting runs <runs>
# times; results.tsv gets one line per run: clip, program, setting, run, wall seconds, first-pass
# seconds.
#
# Settings: gpu (both programs' defaults: CoreML and the hardware encoder, the engine with tile
# mode off), cpu (CPU sessions and libx264, tile mode off), and tiles (the engine's defaults, tile
# mode on). The engine's first pass is timed by the video_benchmark example, 1.12.1's inside
# processVideo.
#
# Needs macOS with the 1.12.1 Homebrew dependencies, like run_legacy_video.sh.
#
#   tools/benchmark/video_speed.sh <onnxruntime library> <models dir> <ffmpeg> <ffprobe> <clips dir> <out dir> [runs]
set -euo pipefail

repo=$(git rev-parse --show-toplevel)
ort=$1
models=$(cd "$2" && pwd)
ffmpeg=$3
ffprobe=$4
clips=$(cd "$5" && pwd)
mkdir -p "$6"
out=$(cd "$6" && pwd)
runs=${7:-3}

rm -rf "$out/src" "$out/build"
mkdir -p "$out/src"
git -C "$repo" archive v1.12.1 | tar -x -C "$out/src"
echo "add_subdirectory(\"$repo/tools/benchmark/legacy\" benchmark-legacy)" >> "$out/src/CMakeLists.txt"
cmake -S "$out/src" -B "$out/build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF \
    -DCLOAKFRAME_SELF_UPDATE=OFF > "$out/configure.log"
cmake --build "$out/build" --target cloakframe_bench_legacy_video_speed > "$out/build.log"
legacy="$out/build/benchmark-legacy/cloakframe_bench_legacy_video_speed"
cargo build --release --locked -p cloakframe-engine --bin cloakframe-cli --example video_benchmark
cli="$repo/target/release/cloakframe-cli"
example="$repo/target/release/examples/video_benchmark"

# No 4K clip is published under a license the benchmark takes, so the street clip is scaled up.
if [[ ! -f "$out/street-4k.mp4" ]]; then
    "$ffmpeg" -v error -i "$clips/street.webm" -an -c:v libx264 -preset fast -crf 18 -g 120 \
        -vf "scale=3840:2160:flags=lanczos,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv,format=yuv420p" \
        "$out/street-4k.mp4"
fi

wall() {
    /usr/bin/time -p "$@" 2> "$out/time.txt" > "$out/stdout.txt" || { cat "$out/time.txt" >&2; exit 1; }
    awk '/^real/ {print $2}' "$out/time.txt"
}

engine() {
    local video=$1 setting=$2 gpu=on tiles=off encode=on
    case $setting in
        cpu) gpu=off encode=off ;;
        tiles) tiles=on ;;
    esac
    rm -rf "$out/published" "$out/sessions"
    mkdir -p "$out/published"
    local seconds
    seconds=$(wall "$cli" run --output "$out/published" --ort-library "$ort" --models "$models" \
        --review off --sessions "$out/sessions" --gpu "$gpu" --tiles "$tiles" --videos \
        --ffmpeg "$ffmpeg" --ffprobe "$ffprobe" --hardware-encode "$encode" "$video")
    rm -f "$out"/tracks.*
    wall "$example" --ort-library "$ort" --models "$models" --ffmpeg "$ffmpeg" --ffprobe "$ffprobe" \
        --video "$video" --gpu "$gpu" --tiles "$tiles" --out "$out/tracks.json" > /dev/null
    local first
    first=$(python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print(round(d["detect_seconds"] + d["track_seconds"], 2))' "$out/tracks.json")
    printf '%s\t%s\n' "$seconds" "$first"
}

legacy_run() {
    local video=$1 gpu=on
    [[ $2 == cpu ]] && gpu=off
    rm -f "$out/legacy.mp4"
    local seconds
    seconds=$(wall "$legacy" "$models" "$ffmpeg" "$ffprobe" "$video" "$out/legacy.mp4" "$gpu")
    local first
    # 1.12.1 logs to stdout as well; the result is the JSON line.
    first=$(python3 -I -c 'import json,sys; print(json.loads([l for l in open(sys.argv[1]) if l.startswith("{")][-1])["first_pass"])' "$out/stdout.txt")
    printf '%s\t%s\n' "$seconds" "$first"
}

printf 'clip\tprogram\tsetting\trun\tseconds\tfirst_pass\n' > "$out/results.tsv"
for video in "$clips/street.webm" "$clips/road.webm" "$clips/crowd.mp4" "$out/street-4k.mp4"; do
    clip=$(basename "${video%.*}")
    for run in $(seq "$runs"); do
        for setting in gpu cpu; do
            times=$(legacy_run "$video" "$setting")
            printf '%s\t1.12.1\t%s\t%s\t%s\n' "$clip" "$setting" "$run" "$times" >> "$out/results.tsv"
            times=$(engine "$video" "$setting")
            printf '%s\tengine\t%s\t%s\t%s\n' "$clip" "$setting" "$run" "$times" >> "$out/results.tsv"
        done
        times=$(engine "$video" tiles)
        printf '%s\tengine\ttiles\t%s\t%s\n' "$clip" "$run" "$times" >> "$out/results.tsv"
    done
done
echo "results in $out/results.tsv"
