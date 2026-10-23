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

CloakFrame 1.12.1 was the last release of the C++ implementation; its source is
kept at the `v1.12.1` tag. This branch is the Rust rewrite described in
[docs/RUST_PORT_PLAN.md](docs/RUST_PORT_PLAN.md). The workspace builds with the
toolchain pinned in `rust-toolchain.toml`, which `rustup` installs on first use.
The JPEG and WebP codecs (libjpeg-turbo, libwebp) are built from source during
the build, so a C compiler and CMake are also needed. On x86, NASM enables
libjpeg-turbo's SIMD code; builds without it work but are slower.

```bash
cargo build --workspace
cargo test --workspace
```

The detector tests, the photo P-1 tests and the command-line photo tests run
the pinned ONNX Runtime on hashed models and sample images, and the media tests
run the pinned FFmpeg; none of them are in the repository. Fetch them once and
point the tests at them (use `linux-x86_64`, or `windows-ml windows-x64`, on other platforms; there
FFmpeg is `ffmpeg <platform>` and ffprobe sits next to it):

```bash
export CLOAKFRAME_ORT_LIBRARY=$(python3 -I tools/fetch_native.py onnxruntime macos-arm64 ~/.cache/cloakframe-native)
export CLOAKFRAME_TEST_ASSETS=$(python3 -I tools/fetch_test_assets.py ~/.cache/cloakframe-test-assets)
export CLOAKFRAME_FFMPEG=$(python3 -I tools/fetch_native.py ffmpeg macos-arm64 ~/.cache/cloakframe-native)
export CLOAKFRAME_FFPROBE=$(python3 -I tools/fetch_native.py ffprobe macos-arm64 ~/.cache/cloakframe-native)
export ORT_DISABLE_TELEMETRY=1
```

Without them those tests fail rather than skip.

On Windows the tests also need a folder on a FAT32 volume, as on a camera SD
card, in `CLOAKFRAME_FAT32_DIR`. CI makes one from a VHD with `diskpart` (see
`.github/workflows/ci.yml`); a FAT32 USB stick or card works as well.

With the same files the command-line tool processes photos:

```bash
cargo run -p cloakframe-engine --bin cloakframe-cli -- run --output out --review off \
  --ort-library "$CLOAKFRAME_ORT_LIBRARY" --models "$CLOAKFRAME_TEST_ASSETS" photos/
```

It detects faces with YOLO5Face-n; `--faces yunet` picks the other face model
and `--plates on` adds license plates. Models run on the GPU where the build
can (CoreML on macOS, DirectML on Windows); `--gpu off` keeps them on the CPU.
Without `--models` it uses the app's model folder: `cloakframe-cli models` lists
the models there with their terms, and `cloakframe-cli models fetch yunet plates`
downloads them (YOLO5Face-n, which is for non-commercial use only, also needs
`--accept-terms`). The command-line tests download the two MIT-licensed models
from GitHub, so they need the network.

With `--videos` it also processes MP4, MOV, M4V and WebM files: every frame is
detected with the same models and settings as photos, and what is tracked is
masked. Each `--mask X,Y,W,H` area, in display pixels, is masked in every frame
as well. Videos cannot be reviewed yet (M5 stage 3), so they need `--review off`:

```bash
cargo run -p cloakframe-engine --bin cloakframe-cli -- run --output out --review off \
  --ort-library "$CLOAKFRAME_ORT_LIBRARY" --models "$CLOAKFRAME_TEST_ASSETS" \
  --videos --ffmpeg "$CLOAKFRAME_FFMPEG" --ffprobe "$CLOAKFRAME_FFPROBE" videos/
```

Videos are written as MP4, H.264 at high quality by default (`--video-codec hevc`,
`--video-quality balanced|space-saver`), on a hardware encoder where one works
(`--hardware-encode off` keeps to software). Each output is decoded in full and
checked before it is published.

Changes to detection also run the detection benchmark: 610 Open Images photos
(Open Images' own copies, about 160 MB) fetched by hash, scored against their
labels and compared with CloakFrame 1.12.1's recorded results. CI runs it on pull
requests that touch detection (`.github/workflows/benchmark.yml`); to run it
locally:

```bash
images=$(python3 -I tools/benchmark/fetch.py ~/.cache/cloakframe-benchmark)
cargo run --release -p cloakframe-engine --example benchmark -- \
  --ort-library "$CLOAKFRAME_ORT_LIBRARY" --models "$CLOAKFRAME_TEST_ASSETS" \
  --manifest tests/fixtures/benchmark/manifest.json --images "$images" --out default.json
python3 -I tools/benchmark/evaluate.py --gate tests/fixtures/benchmark/engine/default.json default.json
```

A change meant to move the default result records the new one into
`tests/fixtures/benchmark/engine/default.json` with `--gpu off`, as CI runs it,
and says why in its record. See [docs/M3_BENCHMARK.md](docs/M3_BENCHMARK.md) for
the set and the other settings.

The parser of ffprobe's report has libFuzzer targets in `fuzz/`, a separate
Cargo project on a pinned nightly (CI's `fuzz.yml` runs them when the parser
changes). `probe_report` mutates raw reports from `fuzz/seeds/`; `probe_fields`
draws the field values and builds the report around them, which reaches the
limits and rules that raw mutation rarely does:

```bash
rustup toolchain install nightly-2026-10-01 --profile minimal
cargo install cargo-fuzz --version 0.13.2 --locked
cargo +nightly-2026-10-01 fuzz run --fuzz-dir fuzz probe_fields -s none -- -max_total_time=120
```

The video decode pipe's throughput is a test of its own that CI prints on each
OS; run it locally with
`cargo test -p cloakframe-engine --test media_decode decode_throughput -- --ignored --nocapture`
(see [docs/M4_DECODE.md](docs/M4_DECODE.md)).

The `spikes/` folders are separate Cargo projects that reproduce the M0
measurements; see their records in `docs/M0_*.md`.

## Make a change

1. Keep changes focused and add regression coverage for changed behavior.
2. Run the checks CI runs:

   ```bash
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --locked -- -D warnings
   cargo clippy --workspace --all-targets --locked --all-features -- -D warnings
   cargo test --workspace --locked --all-features
   cargo deny check
   node --test .github/scripts/update-download-badge.test.js
   ```

3. When a change makes the app behave differently from 1.12.1, add a line to
   `docs/CHANGES_FROM_1.12.md` (1.12.1 behavior, new behavior, reason).
4. Update [README.md](README.md) when user instructions change.

## Safety and compatibility

CloakFrame handles privacy-sensitive media. A change must preserve these core
properties:

- Never modify an input file in place.
- Never silently overwrite an existing output.
- Never upload media or derived personal data.
- Fail closed when processing cannot prove that the requested masking finished.
- Keep updater and downloaded-model integrity checks intact.

Change a pinned toolchain, crate, ONNX Runtime, Windows ML, FFmpeg or model
version only together with the files under [manifests/](manifests/README.md),
the evidence they point to, and CI in the same pull request.

[docs/ROADMAP.md](docs/ROADMAP.md) lists where these properties still have a
gap and what else is planned. Read the part that covers the area you are
touching before working on privacy coverage, the video timeline, publication,
or the updater, and update it when an item changes state.

## Dependencies and licenses

CloakFrame is licensed under the Apache License 2.0. Record any new runtime
dependency and its license in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
Code linked into the application must be under a license compatible with
Apache-2.0, so GPL and AGPL crates are not linked; `cargo deny check` enforces
the crate license allowlist in `deny.toml`. FFmpeg runs as a separate process,
so a GPL build may be bundled, but never one configured with `--enable-nonfree`.

The C++ implementation up to 1.12.1 links Exiv2 (GPL-2.0-or-later), so its
packages were distributed under GPL-3.0-or-later. It is no longer developed.

## Releasing

Releases are cut by the maintainer. The process, the update signing keys and
the release secrets are described in [docs/RELEASING.md](docs/RELEASING.md).
