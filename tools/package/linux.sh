#!/usr/bin/env bash
# The Linux x86_64 AppImage, which is what Velopack installs on Linux (plan 10). Needs Rust, Python,
# cargo-about, nasm, squashfs-tools, the .NET 10 runtime and CLOAKFRAME2_UPDATE_PUBLIC_KEY, the
# update key the app pins (P-7). Build it on Ubuntu 24.04: the app runs only where glibc is at
# least as new as where it was built, and README promises 2.39.
#
# libonnxruntime.so, ffmpeg and ffprobe sit beside the app in usr/bin, where it looks for them
# (crates/cloakframe-app/src/tools.rs). ffmpeg and ffprobe are the build tools/ffmpeg makes,
# GPL-2.0-or-later, with their licenses and build information in FFmpeg/; the release carries their
# corresponding source (plan 10.1).
#
# Usage: tools/package/linux.sh <out>
# Writes the AppImage, the full package and releases.linux.json to <out> and prints the AppImage's
# path. tools/package/sign_updates.py signs the package for the feed.
set -euo pipefail

out=$1
root=$(cd "$(dirname "$0")/../.." && pwd)
target=x86_64-unknown-linux-gnu
native=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/cloakframe-native

version=$(python3 -I -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["workspace"]["package"]["version"])' "$root/Cargo.toml")

cargo build --release --locked -p cloakframe-app --features require-simd --target "$target" \
    --manifest-path "$root/Cargo.toml" >&2
exe=${CARGO_TARGET_DIR:-$root/target}/$target/release/cloakframe

ort=$(python3 -I "$root/tools/fetch_native.py" onnxruntime linux-x86_64 "$native")
ort_root=$(cd "$(dirname "$ort")/.." && pwd)
ffmpeg_dir=$(dirname "$(python3 -I "$root/tools/fetch_native.py" ffmpeg linux-x86_64 "$native")")
vpk_root=$(python3 -I "$root/tools/fetch_native.py" vpk linux-x86_64 "$native")

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp "$exe" "$stage/cloakframe"
cp "$ort" "$stage/libonnxruntime.so"
cp "$ffmpeg_dir/ffmpeg" "$ffmpeg_dir/ffprobe" "$stage/"
mkdir "$stage/FFmpeg"
cp "$ffmpeg_dir/BUILDINFO.txt" "$ffmpeg_dir"/licenses/* "$stage/FFmpeg/"
cp "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.txt" "$stage/"
cp "$ort_root/LICENSE" "$stage/ONNXRUNTIME_LICENSE.txt"
cp "$ort_root/ThirdPartyNotices.txt" "$stage/ONNXRUNTIME_THIRD_PARTY_NOTICES.txt"
cargo about generate --locked --manifest-path "$root/crates/cloakframe-app/Cargo.toml" \
    --config "$root/tools/about/about.toml" --target "$target" \
    --output-file "$stage/RUST_CRATE_NOTICES.txt" "$root/tools/about/notices.hbs" >&2
"$stage/ffmpeg" -hide_banner -version >/dev/null
"$stage/ffprobe" -hide_banner -version >/dev/null

mkdir -p "$out"
out=$(cd "$out" && pwd)
# The same packId as Windows. 1.12.1's was CloakFrame, so neither touches the other's packages.
dotnet "$vpk_root/tools/net10.0/any/vpk.dll" --yes --skip-updates pack \
    --packId CloakFrameApp \
    --packVersion "$version" \
    --packDir "$stage" \
    --mainExe cloakframe \
    --packTitle 'CloakFrame 2' \
    --packAuthors Nyabi \
    --icon "$root/assets/icons/cloakframe-512.png" \
    --categories 'Graphics;Photography' \
    --runtime linux-x64 \
    --channel linux \
    --delta None \
    --outputDir "$out" >&2
echo "$out/CloakFrameApp.AppImage"
