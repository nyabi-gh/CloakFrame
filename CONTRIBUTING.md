# Contributing to CloakFrame

Thank you for helping improve CloakFrame. Bug reports and focused pull requests
are welcome.

## Report a problem

Open a GitHub issue and include:

- What you were doing and what happened
- What you expected instead
- CloakFrame version and operating system
- Whether the input was an image or video and which detector was selected
- Relevant activity-log output with private paths or filenames removed

Do not attach private photos or videos unless you have explicitly decided they
are safe to share. A small synthetic sample is preferred for reproductions.
Report security problems privately, as described in [SECURITY.md](SECURITY.md).

## Build and test

CloakFrame uses CMake 3.24 or later and C++23. The checked-in presets use
Ninja, write build output under `out/build/`, export `compile_commands.json`,
and enable tests.

### Dependencies

- Qt 6.8.1 or later: Core, Gui, Widgets, and Network
- Qt Image Formats, for its TIFF and WebP plugins. Without them those inputs
  are skipped and the metadata test fails
- OpenCV 4.10 or later, including OpenCV 5: core, dnn, imgcodecs, imgproc,
  and objdetect
- ONNX Runtime
- spdlog
- libsodium, which verifies update signatures
- Exiv2 (optional, enables metadata preservation)
- Qt Linguist Tools (optional, embeds non-English translations)
- Qt SVG (optional, renders the settings icon as SVG)
- FFmpeg and FFprobe at runtime for video processing

Official releases use Qt 6.11.2, OpenCV 5.0.0 and ONNX Runtime 1.29.0 (1.24.4
with DirectML on Windows), with spdlog and Exiv2 from vcpkg at the commit in
`VCPKG_COMMIT`; every version is pinned in the workflows. The Linux AppImage is
built on Ubuntu 24.04, whose glibc is the oldest it runs on, with OpenCV's own
image codecs and Qt Image Formats built from source. The macOS build installs its
dependencies with `.github/scripts/install_macos_dependencies.sh`, not Homebrew.

The detection models are runtime data, not build dependencies. They are not
bundled or committed. The application downloads them on first use, or you can
place these files in `models/` before launching:

- `yolov5n_face.onnx`
- `face_detection_yunet_2023mar.onnx`
- `yolo-v9-t-512-license-plates-end2end.onnx`

Only load custom ONNX models from sources you trust.

### Configure, build, and test

With dependencies available to CMake:

```bash
cmake --preset debug
cmake --build --preset debug --parallel
ctest --preset debug --parallel
```

Use the `release` preset for an optimized build. If Ninja is unavailable, the
equivalent generator-independent commands are:

```bash
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug \
  -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

The video I/O tests run a real FFmpeg round trip and report themselves as
skipped when FFmpeg is unavailable.

Standard output follows the same policy as the log file: it carries only
filename-free diagnostics unless detailed logging is turned on in Settings.
Turn it on to see the activity log in a terminal.

### macOS

Install the development dependencies with Homebrew:

```bash
brew install cmake ninja qt opencv onnxruntime spdlog libsodium exiv2 ffmpeg
cmake --preset debug
cmake --build --preset debug --parallel
open out/build/debug/CloakFrame.app
```

When Qt Linguist Tools are missing, the application still builds but embeds
only the English interface. Reconfigure after installing a Qt distribution
that includes the tools.

### Windows

Run from a Visual Studio developer PowerShell. Make Qt, OpenCV, ONNX Runtime,
spdlog, libsodium, and optional Exiv2 discoverable through `CMAKE_PREFIX_PATH` and
`ONNXRUNTIME_ROOT`:

```powershell
cmake --preset release `
  -DCMAKE_PREFIX_PATH="C:\Qt\6.11.2\msvc2022_64;C:\opencv\build;C:\vcpkg\installed\x64-windows-static-md" `
  -DONNXRUNTIME_ROOT="C:\onnxruntime-directml"
cmake --build --preset release --parallel
ctest --preset release
```

Official Windows packages use `Microsoft.ML.OnnxRuntime.DirectML`, with its
headers and libraries staged under an `include`/`lib` root and `DirectML.dll`
placed next to `onnxruntime.dll`. Pass the DLL to CMake with
`-DCLOAKFRAME_DIRECTML_DLL=C:\path\to\DirectML.dll` when staging a package.

### Linux

On Ubuntu, install the base packages with:

```bash
sudo apt install cmake ninja-build build-essential pkg-config ffmpeg patchelf \
  libjpeg-dev libpng-dev libtiff-dev libwebp-dev \
  libspdlog-dev libsodium-dev libexiv2-dev
```

`patchelf` is only needed to stage a package. Qt otherwise rewrites plugin
RPATHs with `file(RPATH_SET)`, which can only replace an entry in place and
fails on plugins whose own RUNPATH is shorter than the deployed path.
`CLOAKFRAME_APPIMAGE_LAYOUT` therefore requires it.

Install a sufficiently recent Qt and OpenCV separately when distribution
packages are older. ONNX Runtime is detected through `pkg-config
libonnxruntime`; otherwise provide an extracted release package:

```bash
cmake --preset release \
  -DONNXRUNTIME_ROOT=/path/to/onnxruntime-linux-x64
cmake --build --preset release --parallel
./out/build/release/CloakFrame
```

On Arch Linux, a CPU development environment can be installed with:

```bash
yay -S --needed base-devel cmake ninja pkgconf qt6-base qt6-tools qt6-svg qt6-imageformats \
  opencv onnxruntime-cpu spdlog libsodium exiv2 ffmpeg
```

Use `onnxruntime-opt-cuda` for supported NVIDIA GPUs or `onnxruntime-rocm` for
supported AMD GPUs. These ONNX Runtime variants conflict, so install only one
and reconfigure after changing it. The official AppImage currently uses CPU
inference.

### Developer checks

The repository `.clang-format` and `.clang-tidy` files are CI policy.
Configuring with LLVM tools available adds these targets:

```bash
cmake --preset debug
cmake --build --preset debug --target cloakframe_format
cmake --build --preset debug --target cloakframe_format_check
cmake --build --preset debug --target cloakframe_tidy --parallel
```

If LLVM is installed outside `PATH`, specify the tools during configuration:

```bash
cmake --preset debug \
  -DCLOAKFRAME_CLANG_FORMAT=/path/to/clang-format \
  -DCLOAKFRAME_CLANG_TIDY=/path/to/clang-tidy \
  -DCLOAKFRAME_RUN_CLANG_TIDY=/path/to/run-clang-tidy
```

Set `-DCLOAKFRAME_WARNINGS_AS_ERRORS=ON` to match the compiler-warning CI gate.
The `cloakframe_tidy` target builds every analysis input first and uses the
exported compilation database.

### Useful CMake options

| Option | Default | Purpose |
| --- | --- | --- |
| `BUILD_TESTING` | `ON` | Build and register the test suite |
| `CLOAKFRAME_SELF_UPDATE` | `ON` | Include Velopack or Sparkle self-update support |
| `CLOAKFRAME_SPARKLE_PUBLIC_KEY` | empty | Pin the macOS update signing key so Sparkle requires an EdDSA signature |
| `CLOAKFRAME_UPDATE_PUBLIC_KEY` | empty | Pin the Windows and Linux update signing key so updates require an Ed25519 signature |
| `CLOAKFRAME_WARNINGS_AS_ERRORS` | `OFF` | Promote compiler warnings to errors |
| `CLOAKFRAME_FFMPEG_DIR` | empty | Bundle `ffmpeg` and `ffprobe` from this directory during installation |
| `CLOAKFRAME_DIRECTML_DLL` | empty | Bundle DirectML with a Windows installation |
| `CLOAKFRAME_APPIMAGE_LAYOUT` | `OFF` | Add the top-level links required by a staged Linux AppDir |

Pass `-DCLOAKFRAME_SELF_UPDATE=OFF` for distribution packaging that must not
embed the project's updater. The app then links users to GitHub Releases.

## Make a change

1. Build the project and run the tests described above.
2. Keep changes focused and add regression coverage for changed behavior.
3. Run the formatting and static-analysis checks.
4. Update the Korean and Japanese translations in `translations/` when
   user-visible strings change. Regenerate the catalogs with the
   `update_translations` target, then fill in the new entries and delete the
   ones lupdate marks vanished. The `translation_quality` test fails on an
   unfinished entry, a vanished one, or one left identical to the English
   source.
5. Add a line under `## Unreleased` in [CHANGELOG.md](CHANGELOG.md) when the
   change is something people using the app would notice. Write it for them,
   not as a commit subject; build and CI changes stay out.
6. Update [README.md](README.md) when user instructions change.

The local quality-gate commands are:

```bash
cmake --preset debug -DCLOAKFRAME_WARNINGS_AS_ERRORS=ON
cmake --build --preset debug --parallel
ctest --preset debug --parallel
cmake --build --preset debug --target cloakframe_format_check
cmake --build --preset debug --target cloakframe_tidy --parallel
node --test .github/scripts/update-download-badge.test.js
```

Use the `cloakframe_format` target to apply formatting.

## Safety and compatibility

CloakFrame handles privacy-sensitive media. A change must preserve these core
properties:

- Never modify an input file in place.
- Never silently overwrite an existing output.
- Never upload media or derived personal data.
- Fail closed when processing cannot prove that the requested masking finished.
- Keep updater and downloaded-model integrity checks intact.

Avoid changing supported operating-system, Qt, OpenCV, ONNX Runtime, model, or
FFmpeg baselines without updating CI, packaging scripts, and documentation in
the same pull request.

[docs/ROADMAP.md](docs/ROADMAP.md) lists where these properties still have a
gap and what else is planned. Read the part that covers the area you are
touching before working on privacy coverage, the video timeline, publication,
or the updater, and update it when an item changes state.

## Dependencies and licenses

Record any new runtime dependency and its license in
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt). Dependencies linked into the
application must be compatible with `GPL-3.0-or-later`, and redistributable
files must be included by every affected packaging path. Bundled FFmpeg builds
must be GPL-3.0-or-later without `--enable-nonfree`;
`.github/scripts/check_ffmpeg_license.sh` enforces this in the release workflow.

## Releasing

Releases are cut by the maintainer. The process, the update signing keys and
the release secrets are described in [docs/RELEASING.md](docs/RELEASING.md).
