#!/usr/bin/env bash
# Checks a build of tools/ffmpeg/build.sh on the OS it is for: it opens only local files and
# pipes, reads only the containers the engine takes, has the encoders the engine picks from, and
# turns a few frames into an MP4 that it can read back.
#
# Usage: tools/ffmpeg/check.sh <platform> <folder with ffmpeg and ffprobe>
set -euo pipefail

platform=$1
dir=$2
exe=
[ "$platform" = windows-x64 ] && exe=.exe
ffmpeg=$dir/ffmpeg$exe
ffprobe=$dir/ffprobe$exe

fail() {
    echo "check: $*" >&2
    exit 1
}

# The names `ffmpeg -<list>` gives after its header, sorted and space separated.
listed() {
    "$ffmpeg" -hide_banner "-$1" | tr -d '\r' | sed -n '/^ *--*$/,$p' | sed 1d | awk '{print $2}' | sort | tr '\n' ' '
}

protocols=$("$ffmpeg" -hide_banner -protocols | tr -d '\r' | sed -n '/^Input:/,$p' | grep -v ':$' | awk '{print $1}' | sort -u | tr '\n' ' ')
[ "$protocols" = "file pipe " ] || fail "protocols are '$protocols', not file and pipe"

demuxers=$(listed demuxers)
[ "$demuxers" = "matroska,webm mov,mp4,m4a,3gp,3g2,mj2 rawvideo " ] ||
    fail "demuxers are '$demuxers'"

encoders=$(listed encoders)
wanted="libx264 libx265 aac"
case $platform in
    macos-arm64) wanted+=" h264_videotoolbox hevc_videotoolbox" ;;
    linux-x86_64) wanted+=" h264_nvenc hevc_nvenc" ;;
    windows-x64) wanted+=" h264_nvenc hevc_nvenc h264_qsv hevc_qsv" ;;
    *) fail "unknown platform $platform" ;;
esac
for name in $wanted; do
    case " $encoders" in
        *" $name "*) ;;
        *) fail "no $name encoder among '$encoders'" ;;
    esac
done

"$ffmpeg" -hide_banner -buildconf | grep -q -- '--enable-gpl' || fail "not a GPL build"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Git Bash on Windows rewrites arguments that start with /, not ones that start with file:.
command -v cygpath >/dev/null && work=$(cygpath -m "$work")
# Three black 64×48 frames through the engine's output filter (media/encode.rs) and both
# software encoders, then read back in full as the output check does (media/verify.rs).
for encoder in libx264 libx265; do
    head -c $((64 * 48 * 3 * 3)) /dev/zero |
        "$ffmpeg" -hide_banner -loglevel error -nostdin -protocol_whitelist pipe \
            -f rawvideo -pix_fmt rgb24 -s 64x48 -framerate 30 -i pipe:0 \
            -vf 'setsar=1/1,scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv' \
            -c:v "$encoder" -f mp4 "$work/$encoder.mp4"
    frames=$("$ffprobe" -hide_banner -loglevel error -protocol_whitelist file -count_frames \
        -select_streams v:0 -show_entries stream=nb_read_frames -of csv=p=0 "file:$work/$encoder.mp4" | tr -d '\r')
    [ "$frames" = 3 ] || fail "$encoder: ffprobe read $frames frames, not 3"
    "$ffmpeg" -hide_banner -loglevel error -nostdin -xerror -protocol_whitelist file \
        -i "file:$work/$encoder.mp4" -map 0 -f null - || fail "$encoder: the output does not decode"
done

echo "check: $platform build is good"
"$ffmpeg" -hide_banner -version | head -1
