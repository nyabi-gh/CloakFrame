# Security Policy

## Supported versions

Fixes go into CloakFrame 2, the Rust rewrite on `main`, and ship in its next preview or
release. CloakFrame 1.12.1 is the last release of the C++ implementation and no longer
receives fixes. Reports about 1.12.1 are still welcome when the same problem may exist in
CloakFrame 2. There are no maintenance branches, so upgrading is the supported way to
receive a fix.

## Reporting a vulnerability

Report privately through GitHub:
[**Report a vulnerability**](https://github.com/nyabi-gh/CloakFrame/security/advisories/new).

Do not open a public issue for a security problem. Public issues are the right
place for ordinary bugs, as described in [CONTRIBUTING.md](CONTRIBUTING.md).

Include what you were doing, what happened, what you expected instead, the
CloakFrame version, and the operating system. Add the reproduction steps and, if
you have one, a proof of concept.

Do not attach private photos or videos. If the report is about media that was
handled incorrectly, reproduce it with a synthetic sample and describe the
property of the original that mattered — resolution, codec, rotation, frame
rate, or the position of the region that was missed.

## What counts as a vulnerability

CloakFrame masks faces and licence plates in photos and video, entirely on the
user's own machine. A report is in scope when it breaks one of the properties
[CONTRIBUTING.md](CONTRIBUTING.md#safety-and-compatibility) requires the project
to hold:

- An output file exposes a face or plate that the run reported as covered.
- Personal data survives a completed run in an output file, a temporary file, a log,
  or embedded metadata.
- Media or data derived from it leaves the machine.
- An input file is modified in place, or an existing output is silently
  overwritten.
- The updater accepts a package that the public key pinned into the build did
  not sign, or a downloaded model passes its integrity check when it should not.
- Opening a media file or an update leads to code execution.

## Out of scope

- **A detector missing something it could not find.** CloakFrame is built to
  fail closed: when it cannot prove a region was covered, it says so and leaves
  the item for review instead of reporting it as done. A run that tells you it
  did not cover something is working as designed. A run that claims coverage it
  did not deliver is the first item in the list above, and is in scope.
- **Another process running as the same user, or another account on the same
  machine, that can already write to the user's files or to CloakFrame's data
  and cache folders.** This sits outside the threat model by decision. A model
  is still checked against its pinned hash when it is loaded.
- **Vulnerabilities in ONNX Runtime, FFmpeg, egui, wgpu, or the other libraries
  and crates CloakFrame bundles (see
  [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt)) themselves.** Report those
  upstream. Do tell us if the way CloakFrame uses one of them makes an upstream
  issue reachable, or if a bundled version needs to move.

## What to expect

This is a single-maintainer project, so a first reply may take several days.
You will be told whether the report is accepted, and a fix ships in the next
preview or release. Please give the fix time to reach users before writing
publicly about it.
