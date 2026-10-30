# Releasing CloakFrame

Maintainer notes for publishing CloakFrame 2. Contributors do not need any of this; see
[CONTRIBUTING.md](../CONTRIBUTING.md) to build and test.

CloakFrame 2 has no stable release yet. M7 of the [plan](RUST_PORT_PLAN.md) (8, 10) builds
the installers, signing and the update feed; so far the releases are previews for macOS and
Windows. The `release.yml` workflow that released 1.12.1 and earlier, with the CMake build
and the deleted `CloakFrame-updates` repository, is kept at the `v1.12.1` tag, and its notes
are in this file's history: `git show 78bff36:docs/RELEASING.md`.

## Preview

The [release workflow](../.github/workflows/release.yml) builds two packages, neither of which
has FFmpeg, so both mask photos only ([plan](RUST_PORT_PLAN.md) 10.1):

- The macOS arm64 app in a zip ([macos_preview.sh](../tools/package/macos_preview.sh)),
  ad-hoc signed and not notarized.
- The Windows x64 Velopack installer ([windows.ps1](../tools/package/windows.ps1)). It is not
  Authenticode signed (decision 2026-10-30, no budget), so SmartScreen warns on its first
  run. The portable zip vpk also writes is not released.

On a pull request that changes the packaging, and when run by hand, the workflow only builds
them, and installs, opens and uninstalls the Windows app. On a `v*` tag it also signs the
update package, drafts the pre-release and, after a second approval, publishes the update feed.

1. Set the workspace version in `Cargo.toml` to the preview version, such as
   `2.0.0-preview.1`. The macOS bundle versions take its numbers only (`2.0.0`).
2. Check [PREVIEW_NOTES.md](PREVIEW_NOTES.md): it becomes the release description.
3. Tag the commit on `main` and push the tag. The `release` environment deploys only from
   `v*` tags, so the tag comes before the workflow, which checks that it matches the version:

   ```bash
   git tag v2.0.0-preview.1 origin/main
   git push origin v2.0.0-preview.1
   ```

4. Approve "Sign the update package". The workflow then drafts the pre-release with the zip
   and the installer.
5. Download both from the draft, open them as the notes describe, and publish the draft.
6. Approve "Publish the update feed". It fails while the draft is unpublished; re-run it
   after publishing. It uploads the package and its `.sig` to the `updates` pre-release, and
   then `releases.win.json`, which installed apps read
   ([M7_UPDATES](M7_UPDATES.md)). Every version goes to the one `win` channel, so an installed
   preview updates to the next one. GitHub drops a deployment left unapproved for 30 days.

A pre-release never becomes the latest release, so the README download links keep
pointing at 1.12.1 until 2.0 is stable.

## Signing keys

CloakFrame 2 does not update from 1.x (plan 0.1), so it does not trust the 1.x update
keys. M7 creates a new Sparkle key for macOS and a new update key for Windows and Linux,
and 2.0 pins only those (decision 2026-10-29). The 1.x keys were repository secrets that a
workflow on any branch could read, and no 1.x release follows 1.12.1, so they are retired.

- The eight repository secrets the 1.x release used (`CLOAKFRAME_UPDATE_PRIVATE_KEY`,
  `SPARKLE_ED_PRIVATE_KEY`, `MACOS_CERTIFICATE_P12`, `MACOS_CERTIFICATE_PASSWORD`,
  `APPLE_DEVELOPER_ID`, `MACOS_NOTARY_KEY`, `MACOS_NOTARY_KEY_ID` and
  `MACOS_NOTARY_ISSUER_ID`) and the repository variables `CLOAKFRAME_UPDATE_PUBLIC_KEY`
  and `SPARKLE_ED_PUBLIC_KEY` were deleted on 2026-10-30. M7 sets the Apple certificate and
  a notary key again, in the `release` environment, where the release workflow needs them.
- Create each new key pair on a machine that is not a CI runner, keep an offline copy,
  and delete the exported private key from that machine. Losing a private key means
  installed clients reject every later update, so it has to outlive the machine that
  created it.
- The Velopack update key is an Ed25519 pair. The private half, in PEM, is the
  `CLOAKFRAME2_UPDATE_PRIVATE_KEY` secret; the public half, the base64 of its 32 bytes, is
  the `CLOAKFRAME2_UPDATE_PUBLIC_KEY` variable, which every Windows and Linux release build
  needs ([M7_UPDATES](M7_UPDATES.md)).
  [sign_updates.py](../tools/package/sign_updates.py) signs the full packages a feed lists.
- Store every signing secret in the `release` environment, never in the repository.
  Environment protection, which limits deployments to `v*` tags and requires a reviewer,
  applies only to environment secrets; a repository secret can be read by a workflow
  pushed to any branch. Only the signing and publishing jobs declare
  `environment: release`; the build jobs, which run third-party tools, reference no
  secret. Public keys are not secret and are repository variables.

The Velopack update key pair, and the public half for the variable:

```bash
openssl genpkey -algorithm ed25519 -out update-key.pem
openssl pkey -in update-key.pem -pubout -outform DER | tail -c 32 | base64
```

```bash
gh secret list --env release
gh secret list   # must list none of the signing secrets
```
