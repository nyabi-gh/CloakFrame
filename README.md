<p align="center">
  <img src="assets/cloakframe-512.png" width="112" alt="CloakFrame icon">
</p>

<h1 align="center">CloakFrame</h1>

<p align="center">
  Hide faces and license plates in your photos and videos — entirely on your own computer.
</p>

<p align="center">
  <a href="https://github.com/nyabi-gh/CloakFrame/releases/latest"><img src="https://img.shields.io/github/v/release/nyabi-gh/CloakFrame?style=flat-square&color=6366f1" alt="Latest release"></a>
  <a href="https://github.com/nyabi-gh/CloakFrame/releases"><img src="https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnyabi-gh%2FCloakFrame%2Fdownload-badge%2Fdownloads.json&style=flat-square" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-6366f1?style=flat-square" alt="License"></a>
</p>

## Download

Download CloakFrame from the
[latest release](https://github.com/nyabi-gh/CloakFrame/releases/latest): the `.exe` for
Windows, the `.dmg` for macOS, or the `.AppImage` for Linux.

| Platform | Requires |
| --- | --- |
| **Windows** | Windows 10 or later, 64-bit |
| **macOS** | macOS 15 or later, Apple silicon |
| **Linux** | x86_64 |

On Linux, make the AppImage executable first: `chmod +x CloakFrame-*-Linux-x86_64.AppImage`.

CloakFrame updates itself. It tells you when a new version is out and installs it
only when you agree.

## How it works

1. **Drop** photos, videos, or a whole folder into the window.
2. **Choose** what to hide — faces, license plates, or both — and how: mosaic, blur,
   a solid color, or an image of your own.
3. **Start.** CloakFrame saves anonymized copies to the output folder. Your originals
   are never changed.

Turn on **Review before saving** to check each result, remove false detections, and
add anything the detector missed.

> [!IMPORTANT]
> Automatic detection is not perfect. Check the results before you share them. If a
> run ends with **Review required**, it is not finished yet.

## Privacy

- **Nothing is uploaded.** Photos and videos are processed on your computer only.
- **Metadata is removed.** Saved images carry no location, camera, or other embedded
  data unless you choose to keep the camera, lens, exposure, and capture time. Location
  is a separate choice, and serial numbers and owner or author names are never kept.
  Videos lose their container metadata, subtitles, and GPS tracks.
- **Audio is kept unless you remove it.** Voices and other sounds in a video are not
  anonymized. Turn on **Remove audio from videos** to save videos without sound; when a
  run keeps audio, its result says so.
- **Three kinds of network requests, nothing else:** downloading a detection model the
  first time you use it, checking for a new version at startup (you can turn this off
  in Settings), and downloading an update after you approve it.

## Supported files

| | Formats |
| --- | --- |
| Images | JPG, PNG, BMP, TIFF, WebP |
| Videos | MP4, MOV, M4V, WebM — H.264, HEVC, VP8, or VP9, 8-bit SDR |
| Output video | MP4, H.264 or HEVC |

10-bit and HDR videos are not supported. Video support is in beta: play each result
to the end before sharing it.

CloakFrame uses your GPU where it can and falls back to the CPU automatically. The app
is available in English, Korean, and Japanese.

## FAQ

**Nothing was detected.** Try the other face model, and check that faces or plates are
selected. You can always add missed areas by hand in the review screen.

**A video will not start.** HDR and 10-bit videos are not supported. If you built
CloakFrame yourself, `ffmpeg` and `ffprobe` need to be on your `PATH`.

**Can I use my own model?** Yes — choose an SCRFD `.onnx` file with **Browse…**. Only use
models from sources you trust.

**I used Redactly before.** CloakFrame is its new name. Your settings and downloaded
models carry over automatically.

## License

CloakFrame is free software under the [GNU GPL v3.0 or later](LICENSE).

The detection models are downloaded on first use and keep their own terms. The
recommended face model, **YOLO5Face-n**, was trained on WIDER FACE and is for
**non-commercial use only**. The fast YuNet face model and the license-plate model are
MIT-licensed. Sources and full notices are in
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).

## Contributing

Bug reports and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) to
build and test CloakFrame. Report security issues privately as described in
[SECURITY.md](SECURITY.md).
