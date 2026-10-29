# Roadmap

Open work on CloakFrame 2, the Rust rewrite on `main`, most important first within each
section. The milestones, their completion criteria and the records behind them are in the
[plan](RUST_PORT_PLAN.md) (in Korean), section 8. Items are removed when they are done;
the commit that closes one says so. The 1.x roadmap is in this file's history:
`git show 78bff36:docs/ROADMAP.md`.

**Evidence rule.** An item is closed only when the value it is about reaches the user.
A field that is written but never read, or a fix that covers part of the cases, does not
close it. Check the caller and the full scope before recording a change of state.

## Preview

- **Publish 2.0.0-preview.1.** The [preview workflow](RELEASING.md#preview) drafted the
  photo-only macOS pre-release `v2.0.0-preview.1`. Download its zip, check that Gatekeeper
  blocks it and "Open Anyway" opens it, then publish the draft.

## Packages and updates (M7)

- **Installers.** Velopack installers for Windows and Linux plus an AppImage, and a DMG
  signed with Developer ID and notarized. Each installs, runs offline, updates and
  uninstalls on a clean minimum supported OS, including a Windows 10 1903 VM. The Windows
  installer uses `packId` `CloakFrameApp`, so it neither replaces 1.12.1 (`CloakFrame`) nor
  removes the data in `%LocalAppData%\CloakFrameData`; check both with 1.12.1 installed
  ([review](M0_M6_REVIEW.md) 10).
- **Updates.** Velopack on Windows and Linux, Sparkle on macOS, reading the feed from this
  repository's `updates` pre-release. Updates are signed with new keys
  ([RELEASING](RELEASING.md#signing-keys)) and a downgrade is refused.
- **Video in releases.** Build FFmpeg from pinned source tarballs so its corresponding
  source can be published; until then releases mask photos only (plan 10.1).
- **Notices and terms.** An SBOM and the native manifest with each release, and the
  Microsoft license terms for the Windows ML runtime in the package and the About window.
  Decide whether the installer has to show those terms for consent (plan 10.1).
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
- **Low-severity review items.** The list under "낮음" in [M0_M6_REVIEW](M0_M6_REVIEW.md).
- **Release documents.** Rewrite the README for CloakFrame 2 and write the CHANGELOG from
  [CHANGES_FROM_1.12.md](CHANGES_FROM_1.12.md), with the note that 1.x users install 2
  as a new app.

## After stable (M9)

- **HEIC and HDR input**, the first feature after the stable release.

## Waiting on the maintainer

- **Retire the 1.x signing secrets.** Delete the repository secrets and variables listed
  in [RELEASING](RELEASING.md#signing-keys).
- **Authenticode for the Windows installer.** A cost question. Without it SmartScreen
  warns on first run.

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
