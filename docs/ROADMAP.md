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
  repository-scoped; see [CONTRIBUTING.md](../CONTRIBUTING.md#release-secrets). Both
  update keys have been readable from any branch, so consider rotating them. Clients
  pin the public key, so a rotation needs a transitional release that trusts both keys.
- **Authenticode for the Windows installer.** A cost question. Without it SmartScreen
  warns on first run.
- **Windows Qt 6.11 pin.** Blocked on aqtinstall, which does not yet handle the
  per-architecture Windows repository layout in a release.

## Next — small, privacy and security first

- **Model hashing on the GUI thread** [E-8, OW 8]. Each run hashes the selected model
  once on the GUI thread before starting; a custom model may be 512 MB. Move it to a
  worker and keep the single digest it produces for consent and loading.
- **libsodium notice** [F-1]. Missing from `THIRD_PARTY_NOTICES.txt` although it is
  linked, statically on Windows.
- **Untranslated updater messages** [E-11]. The strings in `SelfUpdaterVelopack.cpp`
  are marked vanished in the catalogs because translation sources are scanned per
  target and the Velopack source is conditional. Add it to the translation sources and
  make `check_translations.py` fail on a vanished entry still present in the source.
  Detector exception text also reaches the activity log in English.
- **Metadata regression tests** [D-1]. Nothing asserts that a default-path image output
  carries no EXIF, XMP, ICC, PNG text or WebP metadata chunks, or that a video output
  drops subtitle, data and attachment streams and per-stream tags.
- **Test timeouts** [OW 5]. Only one test has a `TIMEOUT`; give `add_cloakframe_test` a
  default.
- **Output conflict key** [CF-021]. Case folding in `OutputPlan.cpp` and the scanner's
  visited key follow the compile-target OS, not the filesystem. The no-replace
  publication keeps this fail-closed; what breaks is the promise to refuse before the run
  starts.
- **Soft-edge mask mutex** [OW 4]. `Mosaic.cpp` serializes mask computation across all
  threads, undoing the parallel image pipeline whenever soft edges are on.

## Coverage and honest results

- **Scene-cut boundaries** [A-3]. A gap that spans a detected cut is neither
  interpolated nor reported. Report a cut-boundary gap when tracks before and after the
  cut overlap in space.
- **Audio** [A-8, E-5]. Voices are kept and the result screen does not say so. Add a
  remove-audio option and state it in the completion message.
- **"Done" wording** [E-5]. Say "no reported warnings", and note when review was off.
- **Metadata preservation** [A-6]. The opt-in keeps GPS, serial numbers and owner names
  through a blocklist. Switch to an allowlist and make GPS a separate choice.
- **Skipped file types** [A-10]. Unsupported files in a folder (HEIC, AVIF, GIF, MKV)
  are dropped silently. Show their count by extension.
- **Leftovers** [A-12, A-5]. Publication temp directories in the output folder survive a
  crash, and `stage-roots.txt` keeps up to 32 output folder paths. Sweep the former and
  forget roots that have no stage left; include the file in "Delete logs".
- **Crash dumps** [A-11]. Nothing keeps source pixels out of core dumps or WER reports.
- **GPU errors mid-run** [C-2]. Only model creation falls back to CPU; a later accelerator
  error fails every following file. Retry the file on CPU and add a CPU/accelerator
  parity test.
- **YOLO5Face score** [OW 10]. Uses objectness only; fixing it moves every threshold
  built on that scale.
- **Small details.** Regions under 2 px are returned unblurred and unreported; ellipse
  masks leave box corners uncovered; the thumbnail loader can destroy a persistent model
  index off the GUI thread [C-3]; the folder scan follows NTFS junctions [B-10].

## Review screens and everyday use

- **Video review exits** [E-4, OW 9]. Esc, Cancel all and closing the window discard the
  whole batch without asking; Esc while drawing does the same. Removing a manual track
  has no confirmation or undo.
- **Reprocess from file results** [E-6]. Clears the input list and replaces the output
  folder without asking, and the new folder persists to the next start.
- **Model download failures** [E-7]. No reason is shown and there is no manual install
  path; Yes is the default button.
- **Minor** [E-9]. The progress bar counts files only; a missing FFmpeg is reported per
  video; Space repeats Undo after clicking it; Return in the results table opens the
  unredacted original.
- **Accessibility** [E-10]. No keyboard way to add regions; canvas and timeline lack
  accessible names; placeholder text and timeline marks fail contrast; included and
  excluded tracks differ by color only; Tab order runs right to left in the options row.
- **Features** [R6 and project review]. Full-resolution zoom in review, a masked preview
  in video review, saving and resuming a review, continuous playback, a manual-only mode,
  presets, and a command-line mode. Move model paths and custom models to an advanced
  section of the main window.

## Updates and release engineering

- **Rollback** [B-3]. Signatures cover only the package digest, so an old signed package
  republished under a new version is accepted. Sign version, channel and file name too.
- **Signing job** [B-4, CF-025]. Signing keys are used in jobs that first run unpinned
  Homebrew, vcpkg and dotnet tools. Sign in a separate job, pin vcpkg with a baseline,
  pin Homebrew, and publish an SBOM.
- **AppImage libraries** [B-5]. No RUNPATH on the executable, several libraries taken
  from the host, a CI RUNPATH in the bundled OpenCV, and a smoke test that never loads
  the app. Fix the RUNPATHs and launch the app on other distributions in CI.
- **Sparkle checks after opt-out** [B-6]. Turning update checks off takes effect on macOS
  only after a restart.
- **Release publication** [B-11]. Publish as a draft, then release; staple the app
  bundle as well as the DMG; repin `actions/github-script` to a commit; enable Dependabot
  for actions; `persist-credentials: false`; tag protection.
- **FFmpeg inputs** [B-9, B-11]. Pass `-protocol_whitelist file,pipe` and a `file:`
  prefix; require the checksum manifest in bundled builds.
- **Hardening** [B-11]. `/CETCOMPAT`, `_FORTIFY_SOURCE=3`, stack-clash protection,
  `SetDefaultDllDirectories`; trim macOS image plugins and stop content sniffing in
  `QImageReader`; validate the feed's file name; cap the update-check response size.
- **Notices** [F-2, F-3]. Generate notices from each platform's bundle, ship or offer the
  corresponding source for bundled FFmpeg and Exiv2, and add an About dialog with the
  licenses and model terms.
- **Policy and versions.** macOS may ship without an update key while Windows and Linux
  may not; the Windows ONNX Runtime and the per-platform Qt versions drift apart.

## Code quality

- **Static analysis coverage** [D-4]. clang-tidy never sees the updater or the
  Windows-only code, `HeaderFilterRegex` matches only `/`, and five test targets are
  missing from the tidy dependencies.
- **Sanitizers and fuzzing** [D-2, B-8]. Add an ASan/UBSan job and fuzz `OnnxGraphPatch`
  and the frame-count parsers; the ONNX patcher can allocate far beyond the file size.
  Introduce `-Wconversion` gradually.
- **Structure** [D-3]. Move run-start validation (model digests, consent, input/output
  overlap) out of `MainWindow` where it can be tested. Keep the detection kind (face or
  plate) in `FaceDetection`; express tracking windows in seconds; let the scanner be
  cancelled; return internal validation failures from video review as such, not as a
  user cancel.
- **Measurement.** Detection samples per condition, miss rates, per-stage time and
  memory, and launch checks on the minimum supported OS versions.

## Larger projects

- **Isolate custom ONNX parsing** [CF-016]. Approved model bytes are still parsed and run
  in the main process; this is the largest remaining security gap. A separate process
  with tensor IPC, limits and a sandbox is needed. Dropping SCRFD support would shrink it.
- **Masking strength** [A-7]. Fixed 12-cell mosaics and a truncated blur kernel may not
  stop recognition among known people; lock the grid per track and offer a stronger
  default.
- **Tiled detection** [C-4]. Large photos are scaled to one 640 px pass, so small faces
  are missed.
- **Pass 1 throughput** [OW 6, 7]. Run scene-cut detection on a worker and detection
  through `processOrdered`. Frame striding stays rejected: it changes what gets covered.
- **Offline update signing.** Sign releases with a hardware-held key outside CI.

## Accepted risk

**Shared Velopack cache on Linux** [CF-002]. Velopack keeps packages under `/var/tmp`,
which every account can write. Another hostile local account is outside the threat
model; the ownership and digest checks stay, and the race between the last check and
Velopack opening the file is accepted. Reopen if Velopack changes its cache handling or
multi-user Linux desktops become a supported setup.
