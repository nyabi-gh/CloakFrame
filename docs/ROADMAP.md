# Roadmap

Open work on CloakFrame 2, the Rust rewrite on `main`, most important first within each
section. The milestones, their completion criteria and the records behind them are in the
[plan](RUST_PORT_PLAN.md) (in Korean), section 8. Items are removed when they are done;
the commit that closes one says so. The 1.x roadmap is in this file's history:
`git show 78bff36:docs/ROADMAP.md`.

**Evidence rule.** An item is closed only when the value it is about reaches the user.
A field that is written but never read, or a fix that covers part of the cases, does not
close it. Check the caller and the full scope before recording a change of state.

## Packages and updates (M7)

- **Installers.** Velopack installers for Windows and Linux plus an AppImage, and a DMG
  signed with Developer ID and notarized. Each installs, runs offline, updates and
  uninstalls on a clean minimum supported OS, including a Windows 10 1903 VM. The Windows
  installer ([`tools/package/windows.ps1`](../tools/package/windows.ps1)) installs and
  uninstalls beside 1.12.1 without touching it or `%LocalAppData%\CloakFrameData`
  ([record](M7_WINDOWS.md)). The release workflow builds it, installs, opens and uninstalls
  it, and releases it without Authenticode, so SmartScreen warns on its first run (decision
  2026-10-30). It still needs the VM.
- **Updates.** Velopack on Windows and Linux, Sparkle on macOS, reading the feed from this
  repository's `updates` pre-release. Updates are signed with new keys
  ([RELEASING](RELEASING.md#signing-keys)) and a downgrade is refused. The Windows and Linux
  app checks, downloads and installs an update only when the pinned key signed it, and a
  release build does not build without the key ([record](M7_UPDATES.md)). The release
  workflow signs the Windows package and, after a second approval, publishes the feed;
  2.0.0-preview.1 went out through it ([record](M7_RELEASE.md#첫-릴리스2026-11-01)). It
  still needs an installed preview updating to the next one from that feed, Sparkle on
  macOS, and a run on Linux.
- **GPU acceleration on Linux.** An NVIDIA GPU pack the user downloads after accepting NVIDIA's
  terms (ONNX Runtime's CUDA build and NVIDIA's libraries, 1.2 GB) runs detection on CUDA
  ([record](M7_ACCELERATORS.md)). It still needs a run on a real Linux machine and an older GPU,
  and a legal look at offering NVIDIA's terms this way. The Windows ML catalog providers were
  measured and left out.
- **Video in releases.** Build FFmpeg from pinned source tarballs so its corresponding
  source can be published; until then releases mask photos only (plan 10.1).
- **Notices and terms.** An SBOM and the native manifest with each release. The Microsoft
  license terms for the Windows ML runtime are in THIRD_PARTY_NOTICES, which the package
  and the About window carry; the installer shows no consent page (decision 2026-11-01,
  plan 10.1).
- **Model mirror** (plan 4.6).

## Verification and stable (M8)

- **Comparison with 1.12.1.** Detection quality and speed, long runs, and Windows DML on
  NVIDIA, Intel and AMD hardware.
- **Real operating systems.** Translations, keyboard, screen readers (including Windows
  Narrator), input methods, HiDPI, file dialogs and remote desktop. Check that the app
  makes no network request other than model downloads and updates.
- **Japanese translations.** Messages under `## needs review` in
  `crates/cloakframe-app/assets/i18n/ja.ftl` are machine drafts that need a Japanese
  reviewer.
- **Video detection before the comparison.** Two risks from [M0_M5_REVIEW](M0_M5_REVIEW.md),
  on their own branch before the comparison with 1.12.1 (plan M6 decision 2026-10-25):
  the coarse-view check of video tiles passes on a detection of any score, looser than for
  photos (7), and a moving subject becomes a new track well before 2 s, leaving the frames
  between unmasked and unreported (10).
- **"Image" masking method.** Core and the photo pipeline have it, but the video pipeline,
  the saved settings and sessions, and the app do not. Add them together, design first
  ([M6_UX](M6_UX.md), decision 2026-10-27).
- **CLI options.** The CLI cannot remove audio or set the sensitivity, which the app's
  advanced options can (plan 12: every flow in the UI and the CLI).
- **EXIF fuzz target.** Plan 9.1 fuzzes the EXIF reader and the ffprobe parsers; only the
  ffprobe targets exist ([M4_PROBE](M4_PROBE.md)).
- **Low-severity review items.** The lists under "낮음" in [M0_M6_REVIEW](M0_M6_REVIEW.md) and
  the open rows of 11 in [M0_M5_REVIEW](M0_M5_REVIEW.md) (all but the audio fault and the
  preview panic, which M6 fixed).
- **Release documents.** Rewrite the README for CloakFrame 2 and write the CHANGELOG from
  [CHANGES_FROM_1.12.md](CHANGES_FROM_1.12.md), with the note that 1.x users install 2
  as a new app.

## After stable (M9)

- **HEIC and HDR input**, the first feature after the stable release.

## Waiting on upstream

- **turbojpeg-sys CRT.** Windows debug builds warn `LNK4098` because turbojpeg-sys 1.2.0
  does not pass a CRT to libjpeg-turbo's CMake. Release builds are unaffected. Update
  `Cargo.lock` once a release carries the fix
  ([honzasp/rust-turbojpeg#43](https://github.com/honzasp/rust-turbojpeg/pull/43)) and
  check that Windows CI no longer warns.

## Accepted risk

**1.x clients stay on 1.x.** CloakFrame 2 is not offered to 1.x through their updaters
(plan 0.1), so they keep their version until the user installs 2 by hand.

**Shared Velopack cache on Linux** [CF-002 in the 1.x roadmap]. Velopack keeps packages
under `/var/tmp`, which every account can write. Another hostile local account is outside
the threat model. Check in M7 whether Velopack still does this, and reopen if multi-user
Linux desktops become a supported setup.
