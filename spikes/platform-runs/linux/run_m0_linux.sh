#!/usr/bin/env bash
# M0 checks on Linux x86_64 (Ubuntu 24.04 / glibc 2.39 or newer). See docs/M0_PLATFORM_RUNBOOK.md.
#
#   spikes/platform-runs/linux/run_m0_linux.sh [--no-gui] [--no-sudo] [out dir]
#
# Runs, and writes everything under the out dir (default: ./m0-linux-<host>-<date>):
#   1. ORT spike (api-27) on the official ORT 1.30.0 CPU build against tests/fixtures/detect
#   2. INTER_LINEAR port against OpenCV 5.0 (x86, IPP on and off)
#   3. ORT telemetry: network attempts with ORT_DISABLE_TELEMETRY=1 (allowed network) and with the
#      library default and the 1.12.1 AppImage's ORT (inside a network namespace, so nothing leaves)
#   4. FFmpeg pipe throughput (software, plus VAAPI/NVENC when present)
#   5. egui spike build, automated tests and benchmarks (skipped with --no-gui)
set -euo pipefail

gui=1
use_sudo=1
out=""
for arg in "$@"; do
    case "$arg" in
        --no-gui) gui=0 ;;
        --no-sudo) use_sudo=0 ;;
        *) out="$arg" ;;
    esac
done

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
out=${out:-"$PWD/m0-linux-$(hostname -s)-$(date +%Y%m%d-%H%M)"}
mkdir -p "$out"
out=$(cd "$out" && pwd)
work="$out/work"
mkdir -p "$work"
log() { printf '\n== %s\n' "$*" | tee -a "$out/summary.txt"; }
note() { printf '%s\n' "$*" | tee -a "$out/summary.txt"; }

need() {
    command -v "$1" >/dev/null 2>&1 || { echo "missing: $1 ($2)" >&2; exit 1; }
}
need cargo "install rustup"
need python3 "apt install python3-venv"
need ffmpeg "apt install ffmpeg"
need strace "apt install strace"
need gcc "apt install build-essential"
need curl "apt install curl"
need unsquashfs "apt install squashfs-tools"

log "system"
{
    uname -a
    grep PRETTY_NAME /etc/os-release
    ldd --version | head -1
    lscpu | grep -E "Model name|^CPU\(s\)"
    free -g | head -2
    (lspci 2>/dev/null | grep -Ei "vga|3d|display") || true
    ffmpeg -hide_banner -version | head -1
    rustc --version
} | tee "$out/system.txt" | tee -a "$out/summary.txt"

log "assets"
RUNNER_TEMP="$work" GITHUB_ENV="$work/models.env" bash "$repo/.github/scripts/fetch_test_models.sh"
assets="$work/cloakframe-test-assets"
python3 -I - "$repo/tests/fixtures/assets.json" "$assets" <<'EOF'
import hashlib, json, sys, urllib.request
from pathlib import Path
spec, dest = json.load(open(sys.argv[1])), Path(sys.argv[2])
for a in spec["images"]:
    path = dest / a["name"]
    if not path.exists():
        urllib.request.urlretrieve(a["url"], path)
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != a["sha256"]:
        raise SystemExit(f"hash mismatch for {a['name']}")
    print(a["name"], "OK")
EOF

ort_dir="$work/onnxruntime-linux-x64-1.30.0"
if [ ! -d "$ort_dir" ]; then
    curl -fsSL --retry 3 -o "$work/ort.tgz" \
        https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-x64-1.30.0.tgz
    echo "a5ed5a3cac51fbb2e90da632ae43d19212faaa20e76484e62bcb7c23ddb3b3fd  $work/ort.tgz" | sha256sum -c -
    tar -xzf "$work/ort.tgz" -C "$work"
fi
ort_lib="$ort_dir/lib/libonnxruntime.so.1.30.0"
appimage="$work/CloakFrame-1.12.1-Linux-x86_64.AppImage"
if [ ! -f "$appimage" ]; then
    curl -fsSL --retry 3 -o "$appimage" \
        https://github.com/nyabi-gh/CloakFrame/releases/download/v1.12.1/CloakFrame-1.12.1-Linux-x86_64.AppImage
fi
echo "7bc8ed2ccc8e464bb5186fe37407e46e2ee94a45abde2782062c9023fe17f3b9  $appimage" | sha256sum -c -

log "OpenCV reference (opencv-python-headless 5.0.0.93)"
python3 -m venv "$work/venv"
"$work/venv/bin/pip" -q install "opencv-python-headless==5.0.0.93" "numpy<3" "fonttools==4.67.0"
"$work/venv/bin/python" -I "$repo/spikes/ort/reference/opencv_reference.py" "$work/ref" \
    "$assets/astronaut.png" "$assets/messi5.jpg" "$assets/lena.jpg" "$assets/car_image.webp" \
    "$repo/tests/fixtures/detect/inputs/astronaut-scales.png" "$repo/tests/fixtures/detect/inputs/crowd-280-faces.png" \
    | tee -a "$out/summary.txt"

log "ORT spike build"
(cd "$repo/spikes/ort" && cargo build --release 2>&1 | tail -1)
spike="$repo/spikes/ort/target/release/ort-spike"

log "INTER_LINEAR port vs OpenCV"
for ipp in on off; do
    "$spike" resize-check "$work/ref/resize-ipp-$ipp" | tee -a "$out/resize-$ipp.txt" | cut -c1-400 | tee -a "$out/summary.txt"
done

log "ORT 1.30.0 CPU vs 1.12.1 fixtures"
"$spike" "$assets" "$repo/tests/fixtures" "$work/ref/decoded" "$ort_lib" \
    --fallback-test 5 --simulate-failure-after 3 > "$out/ort-cpu.txt" 2>&1
grep -E "^ORT|differing bytes [1-9]|SUMMARY|FALLBACK|latency|sessions" "$out/ort-cpu.txt" | tee -a "$out/summary.txt"

gcc -O2 -shared -fPIC -o "$work/libnetwatch.so" "$repo/spikes/ort/netwatch/netwatch_linux.c" -ldl

# Two independent counts of attempts to reach an IP address (DNS to the local resolver
# included): libnetwatch (LD_PRELOAD, libc calls) and strace (system calls).
count_netwatch() {
    { grep -E '^(connect|sendto|sendmsg|getaddrinfo) ' "$1" 2>/dev/null || true; } | wc -l
}
count_strace() {
    { grep -E 'connect\(|sendto\(|sendmsg\(' "$1" 2>/dev/null | grep -E 'AF_INET|AF_INET6' || true; } | wc -l
}
run_traced() {
    local label=$1 netns=$2
    shift 2
    local trace="$out/telemetry-$label.strace"
    local watch="$out/telemetry-$label.netwatch"
    local marker="$work/$label.marker"
    rm -f "$watch"
    touch "$marker"
    local traced=(env NETWATCH_LOG="$watch" LD_PRELOAD="$work/libnetwatch.so" strace -f -qq -e trace=connect,sendto,sendmsg -o "$trace" "$@")
    if [ "$netns" = 1 ]; then
        if [ "$use_sudo" = 0 ] && [ "$(id -u)" != 0 ]; then
            note "$label: skipped (needs sudo for a network namespace)"
            return
        fi
        # A fresh network namespace has only a downed loopback: every attempt fails locally and
        # still shows up in the logs.
        if [ "$(id -u)" = 0 ]; then
            unshare --net -- "${traced[@]}" > "$out/telemetry-$label.out" 2>&1 || true
        else
            sudo unshare --net -- sudo -u "$USER" env HOME="$HOME" PATH="$PATH" "${traced[@]}" > "$out/telemetry-$label.out" 2>&1 || true
        fi
    else
        "${traced[@]}" > "$out/telemetry-$label.out" 2>&1 || true
    fi
    local stores
    stores=$({ find "$HOME" -xdev -newer "$marker" -name 'onnxruntime.db*' 2>/dev/null || true; } | tee "$out/telemetry-$label.stores" | wc -l)
    note "$label: attempts libnetwatch $(count_netwatch "$watch") (loaded: $(grep -c 'netwatch loaded' "$watch" 2>/dev/null || echo 0)), strace $(count_strace "$trace"), telemetry store files touched $stores"
    { grep -E '^(getaddrinfo|connect) ' "$watch" 2>/dev/null || true; } | sort | uniq -c | head -5 | tee -a "$out/summary.txt"
}

log "ORT telemetry (each run keeps the environment alive 90 s)"
run_traced disabled-open 0 env ORT_DISABLE_TELEMETRY=1 "$spike" "$assets" "$repo/tests/fixtures" "$work/ref/decoded" "$ort_lib" --linger 90
run_traced default-netns 1 "$spike" "$assets" "$repo/tests/fixtures" "$work/ref/decoded" "$ort_lib" --telemetry-default --linger 90

gcc -O2 -I "$ort_dir/include" -o "$work/ort_untouched" "$repo/spikes/ort/netwatch/ort_untouched.c" -ldl
# The squashfs image starts where the ELF runtime's section headers end; unpacking it directly
# also works where the AppImage cannot be executed (emulated x86, no FUSE).
offset=$(python3 -I -c '
import struct, sys
h = open(sys.argv[1], "rb").read(64)
shoff, = struct.unpack_from("<Q", h, 0x28)
shentsize, shnum = struct.unpack_from("<HH", h, 0x3A)
print(shoff + shentsize * shnum)
' "$appimage")
rm -rf "$work/squashfs-root"
unsquashfs -q -n -o "$offset" -d "$work/squashfs-root" "$appimage" > /dev/null
legacy_ort=$(find "$work/squashfs-root" -name 'libonnxruntime.so*' -type f | head -1)
note "1.12.1 AppImage ORT: ${legacy_ort#"$work"/} ($(strings "$legacy_ort" | grep -c OneCollector) OneCollector strings)"
run_traced v1121-untouched-netns 1 "$work/ort_untouched" "$legacy_ort" "$assets/yolov5n_face.onnx" 90
run_traced v1121-envonly-netns 1 env ORT_DISABLE_TELEMETRY=1 "$work/ort_untouched" "$legacy_ort" "$assets/yolov5n_face.onnx" 90

log "FFmpeg pipe throughput"
(cd "$repo/spikes/ffmpeg-pipe" && cargo build --release 2>&1 | tail -1)
ff="$repo/spikes/ffmpeg-pipe/target/release/ffmpeg-pipe-spike"
"$ff" gen "$work/ffpipe" > /dev/null
hw_decode=none
hw_encoders=""
if [ -e /dev/dri/renderD128 ] && ffmpeg -hide_banner -encoders 2>/dev/null | grep -q h264_vaapi; then
    hw_decode=vaapi
    hw_encoders=h264_vaapi,hevc_vaapi
fi
if command -v nvidia-smi >/dev/null 2>&1 && ffmpeg -hide_banner -encoders 2>/dev/null | grep -q h264_nvenc; then
    hw_decode=cuda
    hw_encoders=h264_nvenc,hevc_nvenc
fi
note "hardware decode: $hw_decode, encoders: ${hw_encoders:-none}"
"$ff" bench "$work/ffpipe" --hw-decode "$hw_decode" --hw-encoders "$hw_encoders" > "$out/ffmpeg-pipe.jsonl" 2> "$out/ffmpeg-pipe.err" || note "ffmpeg bench failed: $(tail -1 "$out/ffmpeg-pipe.err")"
python3 -I - "$out/ffmpeg-pipe.jsonl" <<'EOF' | tee -a "$out/summary.txt"
import json, sys
for line in open(sys.argv[1]):
    d = json.loads(line)
    print(f"{d['clip']:13} {d['case']:64} {d['fps']:7.1f} fps  x{d['realtime_x']:.2f}")
EOF

if [ "$gui" = 1 ]; then
    log "egui spike"
    (cd "$repo/spikes/egui" && PATH="$work/venv/bin:$PATH" ./fetch_fonts.sh > /dev/null && cargo build --release 2>&1 | tail -1)
    (cd "$repo/spikes/egui" && cargo test --release 2>&1 | grep -E "^test |test result: .*[1-9][0-9]* passed|panicked") | tee -a "$out/summary.txt" || true
    egui="$repo/spikes/egui/target/release/egui-spike"
    for renderer in wgpu glow; do
        for bench in upload zoom; do
            "$egui" --renderer "$renderer" --bench "$bench" --frames 300 > "$out/egui-$renderer-$bench.json" 2> "$out/egui-$renderer-$bench.err" \
                && tail -1 "$out/egui-$renderer-$bench.json" | cut -c1-300 | tee -a "$out/summary.txt" \
                || note "egui $renderer $bench failed: $(tail -2 "$out/egui-$renderer-$bench.err" | tr '\n' ' ')"
        done
    done
    note "session: ${XDG_SESSION_TYPE:-unknown}, desktop: ${XDG_CURRENT_DESKTOP:-unknown}"
fi

log "done"
note "results: $out (send summary.txt and the *.json/*.txt files; work/ can be deleted)"
