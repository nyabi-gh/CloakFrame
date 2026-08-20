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

## Prepare a change

1. Build the project and run the tests described in [BUILDING.md](BUILDING.md).
2. Keep changes focused and add regression coverage for changed behavior.
3. Run the repository formatting and static-analysis checks.
4. Update Korean, English, Japanese, and Simplified Chinese UI translations
   when user-visible strings change.
5. Add or update the release notes for a user-visible change. Each version has
   one file per language: `release-notes/<version>.ko.md`, `.en.md`, `.ja.md`,
   and `.zh.md`. CI fails when any of the four is missing for the current
   project version.
6. Update all four READMEs when user instructions change.

The local quality-gate commands are:

```bash
cmake --preset debug -DCLOAKFRAME_WARNINGS_AS_ERRORS=ON
cmake --build --preset debug --parallel
ctest --preset debug --parallel
cmake --build --preset debug --target cloakframe_format_check
cmake --build --preset debug --target cloakframe_tidy --parallel
node --test .github/scripts/update-download-badge.test.js
```

Use the `cloakframe_format` target to apply formatting. The formatter and tidy
targets are available only when their LLVM tools are found during CMake
configuration; see [BUILDING.md](BUILDING.md#developer-checks) for explicit tool
paths.

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

## Dependencies and licenses

Record any new runtime dependency and its license in
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt). Dependencies linked into the
application must be compatible with `GPL-3.0-or-later`, and redistributable
files must be included by every affected packaging path.
