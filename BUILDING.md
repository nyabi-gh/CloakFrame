# Building CloakFrame

CloakFrame uses CMake 3.24 or later and C++23. The checked-in presets use
Ninja, write build output under `out/build/`, export `compile_commands.json`,
and enable tests.

## Dependencies

- Qt 6.8.1 or later: Core, Gui, Widgets, and Network
- OpenCV 4.10 or later, including OpenCV 5: core, dnn, imgcodecs, imgproc,
  and objdetect
- ONNX Runtime
- spdlog
- Exiv2 (optional, enables metadata preservation)
- Qt Linguist Tools (optional, embeds non-English translations)
- Qt SVG (optional, renders the settings icon as SVG)
- FFmpeg and FFprobe at runtime for video processing

Official Windows and Linux releases use Qt 6.10.3 and OpenCV 5.0.0. The
Linux release baseline is Ubuntu 26.04. macOS release builds use the stable
Homebrew packages available to the release workflow.

The detection models are runtime data, not build dependencies. They are not
bundled or committed. The application downloads them on first use, or you can
place these files in `models/` before launching:

- `yolov5n_face.onnx`
- `face_detection_yunet_2023mar.onnx`
- `yolo-v9-t-512-license-plates-end2end.onnx`

Only load custom ONNX models from sources you trust.

## Configure, build, and test

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

## macOS

Install the development dependencies with Homebrew:

```bash
brew install cmake ninja qt opencv onnxruntime spdlog exiv2 ffmpeg
cmake --preset debug
cmake --build --preset debug --parallel
open out/build/debug/CloakFrame.app
```

When Qt Linguist Tools are missing, the application still builds but embeds
only the English interface. Reconfigure after installing a Qt distribution
that includes the tools.

## Windows

Run from a Visual Studio developer PowerShell. Make Qt, OpenCV, ONNX Runtime,
spdlog, and optional Exiv2 discoverable through `CMAKE_PREFIX_PATH` and
`ONNXRUNTIME_ROOT`:

```powershell
cmake --preset release `
  -DCMAKE_PREFIX_PATH="C:\Qt\6.10.3\msvc2022_64;C:\opencv\build;C:\vcpkg\installed\x64-windows-static-md" `
  -DONNXRUNTIME_ROOT="C:\onnxruntime-directml"
cmake --build --preset release --parallel
ctest --preset release
```

Official Windows packages use `Microsoft.ML.OnnxRuntime.DirectML`, with its
headers and libraries staged under an `include`/`lib` root and `DirectML.dll`
placed next to `onnxruntime.dll`. Pass the DLL to CMake with
`-DCLOAKFRAME_DIRECTML_DLL=C:\path\to\DirectML.dll` when staging a package.

## Linux

On Ubuntu, install the base packages with:

```bash
sudo apt install cmake ninja-build build-essential pkg-config ffmpeg patchelf \
  libjpeg-dev libpng-dev libtiff-dev libwebp-dev \
  libspdlog-dev libexiv2-dev
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
yay -S --needed base-devel cmake ninja pkgconf qt6-base qt6-tools qt6-svg \
  opencv onnxruntime-cpu spdlog exiv2 ffmpeg
```

Use `onnxruntime-opt-cuda` for supported NVIDIA GPUs or `onnxruntime-rocm` for
supported AMD GPUs. These ONNX Runtime variants conflict, so install only one
and reconfigure after changing it. The official AppImage currently uses CPU
inference.

## Developer checks

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

## Useful CMake options

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

### macOS update signing key

Sparkle checks an EdDSA signature only when the bundle pins the matching public
key, so `CLOAKFRAME_SPARKLE_PUBLIC_KEY` decides whether macOS updates have a
trust anchor of their own or rely on Apple code signing alone. Configuring
without it prints a warning.

Create the key pair once, on a machine that is not the CI runner:

```bash
build/_deps/sparkle-src/bin/generate_keys
```

`generate_keys` stores the private key in the login keychain and prints the
public key. Export the private key with `generate_keys -x private-key.txt`,
store it as the `SPARKLE_ED_PRIVATE_KEY` repository secret, keep an offline
copy, and delete the exported file. Store the printed public key as the
`SPARKLE_ED_PUBLIC_KEY` repository variable; the release workflow passes it to
CMake and refuses to publish if only one of the two is configured.

Losing the private key means shipped clients reject every later update, so it
has to outlive the machine that created it.

### Windows and Linux update signing key

Velopack checks a package against a hash the release feed supplies, and the same
release supplies the package, so that check says nothing about who published it.
`CLOAKFRAME_UPDATE_PUBLIC_KEY` pins an Ed25519 key the client verifies each
update against, which is the only part of a release an attacker holding the
publishing credentials cannot forge. Configuring without it prints a warning.

This is a separate key from the Sparkle one. Create it off the CI runner:

```bash
openssl genpkey -algorithm ed25519 -out cloakframe-update-key.pem
openssl pkey -in cloakframe-update-key.pem -pubout -outform DER | tail -c 32 | base64
```

Store the printed public key as the `CLOAKFRAME_UPDATE_PUBLIC_KEY` repository
variable and the PEM as the `CLOAKFRAME_UPDATE_PRIVATE_KEY` secret, keep an
offline copy, and delete the local file. The release workflow refuses to run if
only one of the two is configured, and `sign_update_packages.sh` refuses to sign
if the secret's public half is not the pinned key.

Configure the variable and the secret together. A build that pins a key rejects
every update that has no matching signature, so publishing the variable while
the secret is missing would strand existing installs.

## Packaging

Packaging uses the normal CMake install step. Configure with
`CLOAKFRAME_FFMPEG_DIR` pointing to a directory that contains `ffmpeg` and
`ffprobe`, then stage the application with:

```bash
cmake --build build --config Release --parallel
cmake --install build --config Release --prefix /path/to/staging
```

For a Linux AppImage staging tree, configure with
`-DCMAKE_INSTALL_PREFIX=/usr -DCLOAKFRAME_APPIMAGE_LAYOUT=ON` and install with
`DESTDIR` pointing to a directory whose name ends in `.AppDir`.

The release workflow passes the staged Windows and Linux applications to
Velopack, signs and notarizes the macOS application, and publishes the fixed
asset names. It is the canonical description of the signed, pinned
distribution builds.
