#!/usr/bin/env bash
# Builds the ffmpeg and ffprobe that CloakFrame bundles (plan 10.1) from the sources pinned in
# manifests/native.toml (`use = "source"`): the components in components.sh, statically linked,
# with x264, x265 and the platform's hardware encoders. Record: docs/M7_FFMPEG.md.
#
# Usage: tools/ffmpeg/build.sh <platform> <out>
#   macos-arm64   on macOS 15 or later (Xcode command line tools, cmake, pkg-config)
#   linux-x86_64  on Ubuntu 24.04 x86_64, which sets the glibc the build needs (build-essential,
#                 nasm, cmake, pkg-config, git)
#   windows-x64   cross-compiled on Ubuntu 24.04 (mingw-w64, nasm, cmake, pkg-config, git)
# Writes ffmpeg and ffprobe (.exe on Windows), BUILDINFO.txt and the licenses to <out>.
# CLOAKFRAME_NATIVE_CACHE names a folder of downloaded sources (the source bundle's `sources`
# builds offline); without it the sources are fetched into a temporary folder.
set -euo pipefail

platform=$1
mkdir -p "$2"
out=$(cd "$2" && pwd)
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/tools/ffmpeg/components.sh"
revision=$(cat "$root/tools/ffmpeg/revision")

# x264's configure finds its sources by a relative path, which a symlink in the temporary
# folder's path (macOS /var) breaks.
work=$(cd "$(mktemp -d)" && pwd -P)
trap 'rm -rf "$work"' EXIT
prefix=$work/prefix
cache=${CLOAKFRAME_NATIVE_CACHE:-$work/sources}
jobs=$(getconf _NPROCESSORS_ONLN)
export PKG_CONFIG_PATH=$prefix/lib/pkgconfig PKG_CONFIG_LIBDIR=$prefix/lib/pkgconfig

fetch() {
    python3 -I "$root/tools/fetch_native.py" "$1" "$platform" "$cache"
}

x264_flags=(--enable-static --disable-cli --enable-pic --bit-depth=8 --chroma-format=420
    --disable-opencl --disable-lavf --disable-swscale --disable-ffms --disable-gpac --disable-lsmash)
x265_flags=(-DCMAKE_BUILD_TYPE=Release -DENABLE_SHARED=OFF -DENABLE_CLI=OFF -DHIGH_BIT_DEPTH=OFF
    -DENABLE_LIBNUMA=OFF -DENABLE_HDR10_PLUS=OFF)
ffmpeg_flags=(--disable-everything --disable-autodetect --disable-doc --disable-ffplay
    --disable-network --disable-debug --enable-gpl --enable-libx264 --enable-libx265
    --pkg-config-flags=--static --extra-version="cloakframe$revision")
exe=

case $platform in
    macos-arm64)
        # Without the flags on every compile and link, the tools need the macOS they were built on.
        export MACOSX_DEPLOYMENT_TARGET=15.0
        min=-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET
        x264_flags+=(--extra-cflags="$min" --extra-ldflags="$min")
        x265_flags+=(-DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOSX_DEPLOYMENT_TARGET")
        ffmpeg_flags+=(--extra-cflags="$min" --extra-ldflags="$min" --enable-pthreads
            --enable-videotoolbox --enable-encoder=h264_videotoolbox,hevc_videotoolbox)
        ;;
    linux-x86_64)
        ffmpeg_flags+=(--enable-pthreads --enable-ffnvcodec --enable-nvenc
            --enable-encoder=h264_nvenc,hevc_nvenc)
        ;;
    windows-x64)
        host=x86_64-w64-mingw32
        exe=.exe
        export CC=$host-gcc-posix
        x264_flags+=(--host="$host" --cross-prefix="$host-")
        cmake_cross=(-DCMAKE_SYSTEM_NAME=Windows -DCMAKE_SYSTEM_PROCESSOR=x86_64
            -DCMAKE_C_COMPILER="$host-gcc-posix" -DCMAKE_CXX_COMPILER="$host-g++-posix"
            -DCMAKE_RC_COMPILER="$host-windres" -DCMAKE_FIND_ROOT_PATH="$prefix"
            -DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER -DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=ONLY
            -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY)
        x265_flags+=("${cmake_cross[@]}")
        # -static links libgcc, libstdc++ and winpthreads into the executables, so they need
        # nothing beside them but Windows' own DLLs.
        ffmpeg_flags+=(--target-os=mingw32 --arch=x86_64 --cross-prefix="$host-"
            --cc="$host-gcc-posix" --cxx="$host-g++-posix" --pkg-config=pkg-config
            --extra-ldflags=-static --enable-w32threads --enable-d3d11va --enable-dxva2
            --enable-ffnvcodec --enable-nvenc --enable-libvpl
            --enable-encoder=h264_nvenc,hevc_nvenc,h264_qsv,hevc_qsv)
        ;;
    *)
        echo "unknown platform $platform" >&2
        exit 2
        ;;
esac

# x265's CMake reads its version from x265Version.txt only when it finds git, and fails on
# Windows without it.
command -v git >/dev/null || { echo "build.sh needs git (see above)" >&2; exit 2; }

mkdir -p "$work/x264" "$work/x265" "$work/ffmpeg"

x264_src=$(fetch x264-source)
(cd "$work/x264" && "$x264_src/configure" --prefix="$prefix" "${x264_flags[@]}" &&
    make -j"$jobs" && make install-lib-static)

x265_src=$(fetch x265-source)
(cd "$work/x265" && cmake "$x265_src/source" -DCMAKE_INSTALL_PREFIX="$prefix" "${x265_flags[@]}" &&
    cmake --build . -j "$jobs" && cmake --install .)
if [ "$platform" = windows-x64 ]; then
    # x265 lists the compiler's implicit libraries, and mingw's libgcc_s.a is the import library
    # of libgcc_s_seh-1.dll even under -static.
    sed -i 's/ -lgcc_s//g' "$prefix/lib/pkgconfig/x265.pc"
fi

if [ "$platform" != macos-arm64 ]; then
    nv_src=$(fetch nv-codec-headers-source)
    # Its make writes ffnvcodec.pc beside the sources, which stay as fetched.
    cp -R "$nv_src" "$work/nv-codec-headers"
    make -C "$work/nv-codec-headers" PREFIX="$prefix" install
fi

if [ "$platform" = windows-x64 ]; then
    vpl_src=$(fetch libvpl-source)
    mkdir -p "$work/libvpl"
    (cd "$work/libvpl" && cmake "$vpl_src" -DCMAKE_INSTALL_PREFIX="$prefix" \
        -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DBUILD_TESTS=OFF \
        -DBUILD_EXAMPLES=OFF -DINSTALL_DEV=ON -DINSTALL_EXAMPLES=OFF "${cmake_cross[@]}" &&
        cmake --build . -j "$jobs" && cmake --install .)
    # vpl.pc ends its libraries with ${CXX_LIB}, which libvpl 2.17's CMake never sets, so a static
    # link from C misses the C++ runtime the dispatcher needs.
    sed -i 's/^Libs: .*/& -lstdc++/' "$prefix/lib/pkgconfig/vpl.pc"
fi

ffmpeg_src=$(fetch ffmpeg-source)
(cd "$work/ffmpeg" && { "$ffmpeg_src/configure" "${ffmpeg_flags[@]}" "${ffmpeg_components[@]}" ||
    { tail -40 ffbuild/config.log; exit 1; }; } && make -j"$jobs")

cp "$work/ffmpeg/ffmpeg$exe" "$work/ffmpeg/ffprobe$exe" "$out/"
mkdir -p "$out/licenses"
cp "$ffmpeg_src/COPYING.GPLv2" "$out/licenses/GPL-2.0.txt"
cp "$ffmpeg_src/LICENSE.md" "$out/licenses/FFmpeg-LICENSE.md"
cp "$x264_src/COPYING" "$out/licenses/x264-COPYING.txt"
cp "$x265_src/COPYING" "$out/licenses/x265-COPYING.txt"
if [ "$platform" != macos-arm64 ]; then
    # nv-codec-headers carries its MIT license in each header, not in a file of its own.
    sed -n '/Copyright/,/SOFTWARE\./p' "$nv_src/include/ffnvcodec/nvEncodeAPI.h" |
        sed -E 's/^ \*( |$)//' >"$out/licenses/nv-codec-headers-LICENSE.txt"
fi
if [ "$platform" = windows-x64 ]; then
    cp "$vpl_src/LICENSE" "$out/licenses/libvpl-LICENSE.txt"
fi

{
    echo "CloakFrame build $revision of FFmpeg for $platform"
    echo
    for id in ffmpeg-source x264-source x265-source nv-codec-headers-source libvpl-source; do
        python3 -I - "$root/manifests/native.toml" "$id" "$platform" <<'EOF'
import sys, tomllib
manifest = tomllib.loads(open(sys.argv[1], encoding="utf-8").read())
for a in manifest["artifact"]:
    if a["id"] == sys.argv[2] and sys.argv[3] in [p.strip() for p in a["platform"].split(",")]:
        print(f"{a['id']} {a['version']} {a['sha256']} {a['url']}")
EOF
    done
    echo
    echo "configure:"
    grep '^#define FFMPEG_CONFIGURATION' "$work/ffmpeg/config.h" | sed 's/^#define FFMPEG_CONFIGURATION //'
    echo
    echo "compiler:"
    grep '^#define CC_IDENT' "$work/ffmpeg/config.h" | sed 's/^#define CC_IDENT //'
} >"$out/BUILDINFO.txt"
