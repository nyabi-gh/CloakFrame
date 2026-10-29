# Releasing CloakFrame

Maintainer notes for publishing CloakFrame 2. Contributors do not need any of this; see
[CONTRIBUTING.md](../CONTRIBUTING.md) to build and test.

CloakFrame 2 has no stable release yet. Until M7 of the [plan](RUST_PORT_PLAN.md) (8, 10)
builds the installers, signing and the update feed, the only release is the macOS photo
preview below. The `release.yml` workflow that released 1.12.1 and earlier, with the CMake
build and the `CloakFrame-updates` repository, is kept at the `v1.12.1` tag, and its notes
are in this file's history: `git show 78bff36:docs/RELEASING.md`.

## Preview

The [preview workflow](../.github/workflows/preview.yml) builds the macOS arm64 app in
release mode, bundles ONNX Runtime and the notices with
[macos_preview.sh](../tools/package/macos_preview.sh), and attaches the zip to a draft
pre-release. The app is ad-hoc signed and not notarized, and it has no FFmpeg, so it
masks photos only ([plan](RUST_PORT_PLAN.md) 10.1).

1. Set the workspace version in `Cargo.toml` to the preview version, such as
   `2.0.0-preview.1`. The bundle versions take its numbers only (`2.0.0`).
2. Check [PREVIEW_NOTES.md](PREVIEW_NOTES.md): it becomes the release description.
3. Run the workflow from `main` with the matching tag:

   ```bash
   gh workflow run preview.yml --ref main -f tag=v2.0.0-preview.1
   ```

4. Download the zip from the draft, open it on a Mac as the notes describe, and publish
   the draft. Publishing creates the tag on the commit the workflow built.

A pre-release never becomes the latest release, so the README download links keep
pointing at 1.12.1 until 2.0 is stable.

## Signing keys

CloakFrame 2 does not update from 1.x (plan 0.1), so it does not trust the 1.x update
keys. M7 creates a new Sparkle key for macOS and a new update key for Windows and Linux,
and 2.0 pins only those (decision 2026-10-29). The 1.x keys were repository secrets that a
workflow on any branch could read, and no 1.x release follows 1.12.1, so they are retired.

- Delete the eight repository secrets the 1.x release used: `CLOAKFRAME_UPDATE_PRIVATE_KEY`,
  `SPARKLE_ED_PRIVATE_KEY`, `MACOS_CERTIFICATE_P12`, `MACOS_CERTIFICATE_PASSWORD`,
  `APPLE_DEVELOPER_ID`, `MACOS_NOTARY_KEY`, `MACOS_NOTARY_KEY_ID` and
  `MACOS_NOTARY_ISSUER_ID`, and the repository variables `CLOAKFRAME_UPDATE_PUBLIC_KEY`
  and `SPARKLE_ED_PUBLIC_KEY`. M7 sets the Apple certificate and a notary key again where
  the release workflow needs them.
- Create each new key pair on a machine that is not a CI runner, keep an offline copy,
  and delete the exported private key from that machine. Losing a private key means
  installed clients reject every later update, so it has to outlive the machine that
  created it.
- Store every signing secret in the `release` environment, never in the repository.
  Environment protection, which limits deployments to `v*` tags and requires a reviewer,
  applies only to environment secrets; a repository secret can be read by a workflow
  pushed to any branch. Only the signing and publishing jobs declare
  `environment: release`; the build jobs, which run third-party tools, reference no
  secret. Public keys are not secret and are repository variables.

```bash
gh secret list --env release
gh secret list   # must list none of the signing secrets
```
