<p align="center">
  <img src="assets/cloakframe-512.png" width="128" alt="CloakFrame app icon">
</p>

# CloakFrame

[![Latest Release](https://img.shields.io/github/v/release/nyattic/CloakFrame?style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e&color=6366f1)](https://github.com/nyattic/CloakFrame/releases/latest)
[![Downloads](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnyattic%2FCloakFrame%2Fdownload-badge%2Fdownloads.json&style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e)](https://github.com/nyattic/CloakFrame/releases)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-6366f1?style=for-the-badge&logo=gnu&logoColor=white&labelColor=1e1b2e)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-6366f1?style=for-the-badge&logo=qt&logoColor=white&labelColor=1e1b2e)

<p align="center"><a href="README.md">KR</a> · <b>EN</b> · <a href="README.ja.md">JP</a> · <a href="README.zh.md">ZH</a></p>

A desktop app that automatically hides faces and license plates in photos and
videos. Your files are processed entirely on your computer and are never
uploaded to a server.

Drop photos, videos, or folders into the window, choose what to detect, and get
an anonymized copy without changing the original. Hide detections with
pixelation, blur, a solid color, or an image of your choice.

> [!IMPORTANT]
> Automatic detection is not perfect. Enable review before saving and inspect
> the result before sharing it. If the app finishes with **Review required**,
> do not treat the job as complete.

## Download and install

| Platform | Requirements | Download |
| --- | --- | --- |
| Windows | Windows 10 or later, 64-bit | [Download for Windows](https://github.com/nyattic/CloakFrame/releases/latest) |
| macOS | macOS 15 or later, Apple Silicon | [Download for macOS](https://github.com/nyattic/CloakFrame/releases/latest) |
| Linux | x86_64 | [Download for Linux](https://github.com/nyattic/CloakFrame/releases/latest) |

Open the link and choose the file for your platform under **Assets**. Starting
with v1.11.0, use `CloakFrame-Windows-x64-Setup.exe` on Windows,
`CloakFrame-macOS-arm64.dmg` on macOS, and
`CloakFrame-Linux-x86_64.AppImage` on Linux. Releases through v1.10.2 include
the version in each filename. Make the AppImage executable before launching it.

```bash
chmod +x CloakFrame-Linux-x86_64.AppImage
./CloakFrame-Linux-x86_64.AppImage
```

The first time you use a built-in detector, CloakFrame downloads its model once
from GitHub (about 0.23–11 MB) and caches it. Processing can then run offline.

> [!NOTE]
> CloakFrame was previously named Redactly. Existing users keep their settings
> and already-downloaded models after the first launch.

## How to use it

1. Drop photos, videos, or folders into the window.
2. Select **Faces**, **License plates**, or both.
3. For faces, choose **Accurate · YOLO5Face-n** (recommended) or **Fast · YuNet**.
4. Choose a masking style and output folder.
5. Select **Start**.

Original files are never modified. CloakFrame refuses to start if outputs would
collide or an output file already exists, so it never silently overwrites a
previous result.

### Review before saving

Enable **Review before saving** to:

- Remove false detections and add missed regions in photos.
- Inspect face and plate tracks on a video timeline.
- Exclude a false video track for its entire duration.
- Add manual tracks and adjust keyframes as an object moves.

Select a **Tracking gaps (before review)** entry to jump to its first frame.
**Previous gap / Next gap** move to the preceding or following gap start relative
to the current frame, visiting shared start times once. Use the Left/Right arrow
keys or the buttons beside the timeline to move one frame at a time. Times are
relative to the video start and displayed frame numbers begin at 1. Visiting a
gap does not clear its warning.

A job ends as **Done** only when every item was redacted successfully. A failed
or skipped item, or a file saved without a detection, changes the result to
**Review required** and shows a summary. After the run, choose **File results…**
in Activity to filter items needing attention or failures, read the details,
and open input or output folders. Results reset when the next run starts;
files that never started processing are not listed.

## Supported files and processing

- Images: `.jpg` `.jpeg` `.png` `.bmp` `.tif` `.tiff` `.webp`
- Videos: `.mp4` `.mov` `.m4v` `.webm` (H.264/HEVC/VP8/VP9, 8-bit SDR)
- Video output: H.264 (default) or HEVC MP4

Videos are processed in two passes: bidirectional detection and tracking, then
encoding. Compatible audio is kept; other audio is converted to AAC. Rotation
is baked into the pixels and container metadata is removed. CloakFrame rejects
10-bit and HDR input instead of silently reducing its quality.

> [!WARNING]
> Video support is in beta. Watch the full result before sharing it. The Linux
> video path has automated coverage but limited manual testing so far.

CloakFrame uses a GPU when a supported path works and falls back to the CPU.

| Platform | Acceleration path |
| --- | --- |
| macOS | CoreML detection · VideoToolbox encoding |
| Windows | DirectML detection · NVENC/Quick Sync encoding |
| Linux | CUDA/MIGraphX detection in source builds · NVENC/Quick Sync encoding |

The official Linux AppImage currently uses CPU detection. The fast YuNet model
also runs on the CPU on every platform.

## Privacy and network access

Photo and video content never leaves your device. CloakFrame makes only these
network requests, none of which contains an image or personal data:

- Downloading a built-in model the first time it is used.
- Checking GitHub for a newer version at startup.
- Downloading a release after you approve an update.

Turn off the version check under **Settings → Check for updates on startup**.
No update file is downloaded before you approve it.

## Frequently asked questions

### Nothing was detected

Try the other face model and check that the intended face or plate option is
selected. You can add a missed region manually during review.

### Video processing does not start

Video processing needs `ffmpeg` and `ffprobe`. Official packages include them;
source builds need both programs on `PATH`. HDR and 10-bit input is unsupported.

### Can I use my own model?

Choose a SCRFD `.onnx` model with **Browse…**. Load models only from sources you
trust: ONNX files are model inputs executed by native runtime libraries.

## Models and licenses

CloakFrame source code is distributed under the **GNU GPL v3.0 or later**. See
[LICENSE](LICENSE) for the terms. Copyright 2026 Nyabi.

Built-in models are not bundled with the app or repository. The recommended
YOLO5Face-n model should be treated as **non-commercial research only** because
of the WIDER FACE dataset terms. The fast YuNet and license-plate models follow
their respective MIT terms. A custom model keeps the terms of its provider.

Exact model sources, paper citations, and runtime dependency terms are listed
in [Models and third-party notices](docs/MODELS.md) and
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).

## Contributing

See [BUILDING.md](BUILDING.md) to build from source and
[CONTRIBUTING.md](CONTRIBUTING.md) to propose a change. They also cover the
CMake presets, tests, `clang-format`, and `clang-tidy` checks.
