# Releasing CloakFrame

Maintainer notes for cutting a release, staging packages, and managing the
update signing keys. Contributors do not need any of this; see
[CONTRIBUTING.md](../CONTRIBUTING.md) to build and test.

## Cut a release

Pushing a `v<major>.<minor>.<patch>` tag that matches the project version in
`CMakeLists.txt` starts the release. The "Prepare x.y.z" commit that sets the
version also renames `## Unreleased` in [CHANGELOG.md](../CHANGELOG.md) to
`## x.y.z`, reviewed so it reads as release notes (group the changes, lead with
what matters most, and say what users of older versions have to do), and adds a
new empty `## Unreleased` above it. The workflow publishes that
section with a compare link and the SHA-256 of the downloads, and stops before
building when the section is missing or empty.

## What a release publishes

The release workflow passes the staged Windows and Linux applications to
Velopack, signs and notarizes the macOS application, and publishes two releases
under the same tag:

- This repository gets the three downloads, named
  `CloakFrame-<version>-Windows-x64-Setup.exe`, `CloakFrame-<version>-macOS-arm64.dmg`
  and `CloakFrame-<version>-Linux-x86_64.AppImage`, with their SHA-256 in the notes.
- [nyabi-gh/CloakFrame-updates](https://github.com/nyabi-gh/CloakFrame-updates)
  gets what the built-in updaters read: the Velopack feeds, packages and
  signatures, and the Sparkle `appcast.xml`, which points at the DMG here. The
  repository has to stay public, because the app downloads from it without a
  token.

The workflow is the canonical description of the signed, pinned distribution
builds.

## Stage a package locally

Packaging uses the normal CMake install step. Configure with
`CLOAKFRAME_FFMPEG_DIR` pointing to a directory that contains `ffmpeg` and
`ffprobe`, then stage the application with:

```bash
cmake --build build --config Release --parallel
cmake --install build --config Release --prefix /path/to/staging
```

For a Linux AppImage staging tree, configure with
`-DCMAKE_INSTALL_PREFIX=/usr -DCLOAKFRAME_APPIMAGE_LAYOUT=ON` and install with
`DESTDIR` pointing to a directory whose name ends in `.AppDir`.
`.github/scripts/check_linux_launch.sh` starts the result in a distribution
container, as the release does on Ubuntu, Debian, Fedora and Arch:
`docker run --rm -v "$PWD:/w:ro" debian:13 bash /w/.github/scripts/check_linux_launch.sh /w/<AppDir>/AppRun`.

## macOS update signing key

Sparkle checks an EdDSA signature only when the bundle pins the matching public
key, so `CLOAKFRAME_SPARKLE_PUBLIC_KEY` decides whether macOS updates have a
trust anchor of their own or rely on Apple code signing alone. Configuring
without it prints a warning.

Create the key pair once, on a machine that is not the CI runner:

```bash
out/build/release/_deps/sparkle-src/bin/generate_keys
```

The path assumes a macOS `release` preset build, which fetches Sparkle.
`generate_keys` stores the private key in the login keychain and prints the
public key. Export the private key with `generate_keys -x private-key.txt`,
store it as the `SPARKLE_ED_PRIVATE_KEY` secret of the `release` environment
(see [Release secrets](#release-secrets)), keep an offline
copy, and delete the exported file. Store the printed public key as the
`SPARKLE_ED_PUBLIC_KEY` repository variable; the release workflow passes it to
CMake and refuses to publish if only one of the two is configured.

Losing the private key means shipped clients reject every later update, so it
has to outlive the machine that created it.

## Windows and Linux update signing key

Velopack checks a package against a hash the release feed supplies, and the same
release supplies the package, so that check says nothing about who published it.
`CLOAKFRAME_UPDATE_PUBLIC_KEY` pins an Ed25519 key the client verifies each
update against, which is the only part of a release an attacker holding the
publishing credentials cannot forge. The signature covers the channel, version
and file name as well as the package digest, so an older signed package cannot
be offered again as a newer version. Configuring without it prints a warning.

This is a separate key from the Sparkle one. Create it off the CI runner:

```bash
openssl genpkey -algorithm ed25519 -out cloakframe-update-key.pem
openssl pkey -in cloakframe-update-key.pem -pubout -outform DER | tail -c 32 | base64
```

Store the printed public key as the `CLOAKFRAME_UPDATE_PUBLIC_KEY` repository
variable and the PEM as the `CLOAKFRAME_UPDATE_PRIVATE_KEY` secret of the
`release` environment, keep an
offline copy, and delete the local file. The release workflow refuses to run if
only one of the two is configured, and `sign_update_packages.sh` refuses to sign
if the secret's public half is not the pinned key.

Configure the variable and the secret together. A build that pins a key rejects
every update that has no matching signature, so publishing the variable while
the secret is missing would strand existing installs.

## Release secrets

Every signing secret belongs to the `release` environment, never to the
repository. Environment protection, which limits deployments to `v*` tags and
requires a reviewer, applies only to environment secrets; a repository secret
can be read by a workflow pushed to any branch. The jobs that use them
(`check-secrets`, `sign-macos`, `sign-updates` and `release`) already declare
`environment: release`, so the secrets need no other change. The build jobs
reference no secret, so the third-party tools they run never see one; keep
signing out of them.

- `CLOAKFRAME_UPDATE_PRIVATE_KEY`, `SPARKLE_ED_PRIVATE_KEY`
- `MACOS_CERTIFICATE_P12`, `MACOS_CERTIFICATE_PASSWORD`, `APPLE_DEVELOPER_ID`
- `MACOS_NOTARY_KEY`, `MACOS_NOTARY_KEY_ID`, `MACOS_NOTARY_ISSUER_ID`
- `CLOAKFRAME_UPDATES_TOKEN`: a fine-grained personal access token limited to
  the `nyabi-gh/CloakFrame-updates` repository with Contents set to Read and
  write. The publish job uses it to create the release there, and the workflow
  refuses to start a tagged release without it.

```bash
gh secret set CLOAKFRAME_UPDATE_PRIVATE_KEY --env release < cloakframe-update-key.pem
gh secret list --env release
gh secret list   # must not list any of the names above
```

The public keys are not secret and stay repository variables.
