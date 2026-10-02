#!/usr/bin/env bash
set -euo pipefail

# Installs CloakFrame's macOS build dependencies at pinned versions under one prefix, the way
# the Windows and Linux jobs do, so a build does not take whatever Homebrew offers that day.
# Qt comes separately from install-qt-action.
#
# Usage: install_macos_dependencies.sh <prefix>
# Reads OPENCV_VERSION, OPENCV_SOURCE_SHA256, ONNXRUNTIME_VERSION, ONNXRUNTIME_MACOS_SHA256,
# LIBSODIUM_VERSION, LIBSODIUM_SHA256, VCPKG_COMMIT and MACOSX_DEPLOYMENT_TARGET.

prefix="${1:?usage: install_macos_dependencies.sh <prefix>}"
mkdir -p "$prefix"
prefix="$(cd "$prefix" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
jobs="$(sysctl -n hw.ncpu)"

fetch() {
    curl -fsSL "$1" -o "$2"
    echo "$3  $2" | shasum -a 256 -c -
}

ort="onnxruntime-osx-arm64-${ONNXRUNTIME_VERSION}"
if [[ ! -d "$prefix/$ort" ]]; then
    fetch "https://github.com/microsoft/onnxruntime/releases/download/v${ONNXRUNTIME_VERSION}/${ort}.tgz" \
        "$work/onnxruntime.tgz" "$ONNXRUNTIME_MACOS_SHA256"
    tar -xzf "$work/onnxruntime.tgz" -C "$prefix"
fi

# The release tarball carries its configure script; vcpkg's port needs autotools the runner lacks.
if [[ ! -f "$prefix/sodium/lib/libsodium.a" ]]; then
    fetch "https://download.libsodium.org/libsodium/releases/libsodium-${LIBSODIUM_VERSION}.tar.gz" \
        "$work/libsodium.tar.gz" "$LIBSODIUM_SHA256"
    tar -xzf "$work/libsodium.tar.gz" -C "$work"
    (
        cd "$work/libsodium-${LIBSODIUM_VERSION}"
        ./configure --prefix="$prefix/sodium" --disable-shared --enable-static > /dev/null
        make -j"$jobs" > /dev/null
        make install > /dev/null
    )
fi

if [[ ! -d "$prefix/vcpkg/installed/arm64-osx" ]]; then
    git init -q "$prefix/vcpkg"
    git -C "$prefix/vcpkg" fetch -q --depth 1 https://github.com/microsoft/vcpkg.git "$VCPKG_COMMIT"
    git -C "$prefix/vcpkg" checkout -q FETCH_HEAD
    "$prefix/vcpkg/bootstrap-vcpkg.sh" -disableMetrics
    # vcpkg's own triplet targets the build machine's macOS version.
    mkdir -p "$prefix/triplets"
    {
        cat "$prefix/vcpkg/triplets/arm64-osx.cmake"
        echo "set(VCPKG_OSX_DEPLOYMENT_TARGET ${MACOSX_DEPLOYMENT_TARGET})"
    } > "$prefix/triplets/arm64-osx.cmake"
    "$prefix/vcpkg/vcpkg" install --overlay-triplets="$prefix/triplets" \
        spdlog:arm64-osx exiv2:arm64-osx
fi

if [[ ! -d "$prefix/opencv/lib" ]]; then
    fetch "https://github.com/opencv/opencv/archive/refs/tags/${OPENCV_VERSION}.tar.gz" \
        "$work/opencv.tar.gz" "$OPENCV_SOURCE_SHA256"
    tar -xzf "$work/opencv.tar.gz" -C "$work"
    cmake -S "$work/opencv-${OPENCV_VERSION}" -B "$work/opencv-build" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$prefix/opencv" \
        -DCMAKE_INSTALL_LIBDIR=lib \
        -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOSX_DEPLOYMENT_TARGET" \
        -DBUILD_LIST=core,dnn,imgcodecs,imgproc,objdetect \
        -DBUILD_SHARED_LIBS=ON \
        -DOPENCV_FORCE_3RDPARTY_BUILD=ON \
        -DWITH_OPENEXR=OFF \
        -DWITH_OPENJPEG=OFF \
        -DWITH_JASPER=OFF \
        -DBUILD_TESTS=OFF \
        -DBUILD_PERF_TESTS=OFF \
        -DBUILD_EXAMPLES=OFF \
        -DBUILD_opencv_apps=OFF \
        -DBUILD_JAVA=OFF \
        -DBUILD_opencv_python3=OFF \
        -DWITH_FFMPEG=OFF \
        -DWITH_GSTREAMER=OFF \
        -DWITH_QT=OFF > /dev/null
    cmake --build "$work/opencv-build" --parallel
    cmake --install "$work/opencv-build" > /dev/null
fi

{
    echo "CLOAKFRAME_DEPS_PREFIX_PATH=$prefix/vcpkg/installed/arm64-osx;$prefix/sodium;$prefix/opencv"
    echo "ONNXRUNTIME_ROOT=$prefix/$ort"
} >> "${GITHUB_ENV:-/dev/stdout}"
