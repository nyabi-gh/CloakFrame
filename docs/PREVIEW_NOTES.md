A preview of CloakFrame 2, rewritten in Rust, for macOS on Apple silicon (macOS 15 or later). It is not the stable release: there is no automatic update, and the app is not notarized. It masks photos only: FFmpeg, which videos need, is not included yet, so videos are left out.

**Opening it:** unzip, move "CloakFrame Preview" to Applications and open it. macOS blocks the first open of an app that is not notarized; open System Settings > Privacy & Security and choose "Open Anyway".

**Coming from 1.x:** CloakFrame 1.x does not update to 2, so install the preview as a new app. It sits next to CloakFrame 1.12.1, which keeps its settings and keeps working. The preview starts with its own default settings, since settings from 1.12.1 are not carried over, and uses the detection models 1.12.1 already downloaded.

**Detection models** are downloaded from the Detection models section of Advanced Options, each after its terms are accepted.

What changed from 1.12.1 is listed in [CHANGES_FROM_1.12.md](https://github.com/nyabi-gh/CloakFrame/blob/main/docs/CHANGES_FROM_1.12.md). Please report problems as issues, with the graphics line from the About window.
