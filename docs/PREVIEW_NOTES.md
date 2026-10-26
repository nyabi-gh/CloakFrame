A preview of CloakFrame 2, rewritten in Rust, for macOS on Apple silicon (macOS 15 or later). It is not the stable release: settings from 1.12.1 are not carried over, there is no automatic update, and the app is not notarized. It masks photos only: FFmpeg, which videos need, is not included yet, so videos are left out.

**Opening it:** unzip, move "CloakFrame Preview" to Applications and open it. macOS blocks the first open of an app that is not notarized; open System Settings > Privacy & Security and choose "Open Anyway". The preview can sit next to CloakFrame 1.12.1.

**Detection models** are downloaded from the app's Detection Models window, each after its terms are accepted.

What changed from 1.12.1 is listed in [CHANGES_FROM_1.12.md](https://github.com/nyabi-gh/CloakFrame/blob/main/docs/CHANGES_FROM_1.12.md). Please report problems as issues, with the graphics line from the About window.
