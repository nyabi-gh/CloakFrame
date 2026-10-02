# Roadmap

Open work, most important first within each section. Items are removed when they are
done; the commit that closes one says so.

This list replaces three earlier documents: the code, privacy and security review of
v1.11.3 (`REVIEW.md`), the project review (`docs/PROJECT_REVIEW.ko.md`) and the
audit follow-up list (`docs/open-work.md`). The IDs in brackets point into them, with
the evidence and line references: `git log --diff-filter=D -1 -- REVIEW.md` names the
commit that removed them, and `git show <that commit>^:REVIEW.md` prints one.
`A-` to `F-` come from the review, `R` from the project review, `CF-` and `OW`
(open-work section) from the audit list.

**Evidence rule.** An item is closed only when the value it is about reaches the user.
A field that is written but never read, or a fix that covers part of the cases, does not
close it. Check the caller and the full scope before recording a change of state.

## Waiting on the maintainer

- **Move the signing secrets to the `release` environment** [B-1]. All eight are still
  repository-scoped; see [CONTRIBUTING.md](../CONTRIBUTING.md#release-secrets). The
  build jobs no longer reference them, but a workflow on any branch still could. Both
  update keys have been readable from any branch, so consider rotating them. Clients
  pin the public key, so a rotation needs a transitional release that trusts both keys.
- **Try the 1.12.0 packages by hand.** Its release run passed, both update signatures
  verified and the macOS bundled FFmpeg matches its checksum manifests. Still unchecked:
  opening a TIFF and a WebP file in each package, processing a video in the macOS
  package (broken by signing since 85fb6b6), and crash dumps being off in Release builds,
  which no test exercises: on Linux, the running app's `/proc/<pid>` entries are owned by
  root when it is not dumpable.
- **Watch the next release run and try its packages.** The sign-macos and sign-updates
  jobs have never run: a branch dispatch stops at the environment. The build jobs have,
  including the AppImage starting on four distributions and the macOS app starting. No
  AppImage since 1.11.0 started at all, and the macOS app now uses the official Qt,
  ONNX Runtime and a source-built OpenCV instead of Homebrew's, so open a TIFF file in
  the AppImage and process a photo and a video in the DMG, checking that CoreML is used.
- **Authenticode for the Windows installer.** A cost question. Without it SmartScreen
  warns on first run.
- **Release aqtinstall for Qt.** CI installs aqtinstall from a pinned commit because
  3.3.0 cannot find Windows Qt from 6.11 on; return to a PyPI version once one ships
  the fix (miurahr/aqtinstall#1000).

## Review screens and everyday use

- **Features** [R6 and project review]. Full-resolution zoom in review, a masked preview
  in video review, saving and resuming a review, continuous playback, a manual-only mode,
  presets, and a command-line mode. Move model paths and custom models to an advanced
  section of the main window.

## Updates and release engineering

- **macOS rollback** [B-3]. Windows and Linux signatures now cover version, channel and
  file name. Sparkle signs only the DMG and the appcast is not signed; check whether
  Sparkle refuses an older signed DMG listed under a newer version, and sign the feed if
  it does not.
- **SBOM** [B-4, CF-025]. Publish one with each release: the pinned versions from the
  workflows plus the SPDX files vcpkg writes for each port.
- **Sparkle checks after opt-out** [B-6]. Turning update checks off takes effect on macOS
  only after a restart.
- **Release publication** [B-11]. Publish as a draft, then release; staple the app
  bundle as well as the DMG; repin `actions/github-script` to a commit; enable Dependabot
  for actions; `persist-credentials: false`; tag protection.
- **Hardening** [B-11]. `/CETCOMPAT`, `_FORTIFY_SOURCE=3`, stack-clash protection,
  `SetDefaultDllDirectories`; leave image plugins CloakFrame never selects out of the
  bundles; validate the feed's file name; cap the update-check response size.
- **Notices** [F-2, F-3]. Generate notices from each platform's bundle, ship or offer the
  corresponding source for bundled FFmpeg and Exiv2, and add an About dialog with the
  licenses and model terms.
- **Policy and versions.** macOS may ship without an update key while Windows and Linux
  may not; the Windows ONNX Runtime drifts from the others. The Clang-Tidy job still
  takes its dependencies from Homebrew.

## Code quality

- **Static analysis coverage** [D-4]. clang-tidy never sees the updater or the
  Windows-only code, `HeaderFilterRegex` matches only `/`, and five test targets are
  missing from the tidy dependencies.
- **Sanitizers and fuzzing** [D-2, B-8]. Add an ASan/UBSan job and fuzz `OnnxGraphPatch`
  and the frame-count parsers; the ONNX patcher can allocate far beyond the file size.
  Introduce `-Wconversion` gradually.
- **Structure** [D-3]. Move run-start validation (model digests, consent, input/output
  overlap) out of `MainWindow` where it can be tested. Keep the detection kind (face or
  plate) in `FaceDetection`; let the scanner be cancelled.
- **Measurement.** Detection samples per condition, miss rates, per-stage time and
  memory, and launch checks on the minimum supported OS versions.

## Larger projects

- **Isolate custom ONNX parsing** [CF-016]. Approved model bytes are still parsed and run
  in the main process; this is the largest remaining security gap. A separate process
  with tensor IPC, limits and a sandbox is needed. Dropping SCRFD support would shrink it.
- **Masking strength** [A-7]. Fixed 12-cell mosaics and a truncated blur kernel may not
  stop recognition among known people; lock the grid per track and offer a stronger
  default.
- **GPU detection on Linux.** The AppImage ships the CPU build of ONNX Runtime, so the
  CUDA, MIGraphX and ROCm paths in `OrtAcceleration.cpp` never run. A GPU build adds
  hundreds of megabytes and ties the package to the host's CUDA or ROCm version.
- **Tiled detection** [C-4]. Large photos are scaled to one 640 px pass, so small faces
  are missed.
- **Pass 1 throughput** [OW 6, 7]. Run scene-cut detection on a worker and detection
  through `processOrdered`. Frame striding stays rejected: it changes what gets covered.
- **Offline update signing.** Sign releases with a hardware-held key outside CI.

## Accepted risk

**Clients up to 1.11.3 that missed 1.12.0.** Only 1.12.0 carried their update feed in
this repository. macOS clients read `appcast.xml` from the latest release here and find
nothing from 1.12.1 on; Windows and Linux clients scan the ten newest releases and lose
1.12.0 once nine more are out. Neither falls back to a notice, so they stay on their
version until the user downloads a new one.

**Shared Velopack cache on Linux** [CF-002]. Velopack keeps packages under `/var/tmp`,
which every account can write. Another hostile local account is outside the threat
model; the ownership and digest checks stay, and the race between the last check and
Velopack opening the file is accepted. Reopen if Velopack changes its cache handling or
multi-user Linux desktops become a supported setup.
