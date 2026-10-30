A preview of CloakFrame 2, rewritten in Rust, for macOS on Apple silicon (macOS 15 or later) and Windows 10 version 1903 or later (x64). It is not the stable release. It masks photos only: FFmpeg, which videos need, is not included yet, so videos are left out.

**macOS:** unzip, move "CloakFrame Preview" to Applications and open it. The app is not notarized, so macOS blocks its first open; open System Settings > Privacy & Security and choose "Open Anyway". The macOS preview does not update itself.

**Windows:** run the Setup file. It is not code signed, so SmartScreen may warn on its first run; choose "More info", then "Run anyway". It installs for your account only, as "CloakFrame 2", and later previews come to it as updates, each checked against CloakFrame's own signature before it is installed.

**Coming from 1.x:** CloakFrame 1.x does not update to 2, so install the preview as a new app. It sits next to CloakFrame 1.12.1, which keeps its settings and keeps working. The preview starts with its own default settings, since settings from 1.12.1 are not carried over, and uses the detection models 1.12.1 already downloaded.

**Detection models** are downloaded from the Detection models section of Advanced Options, each after its terms are accepted.

What changed from 1.12.1 is listed in [CHANGES_FROM_1.12.md](https://github.com/nyabi-gh/CloakFrame/blob/main/docs/CHANGES_FROM_1.12.md). Please report problems as issues, with the graphics line from the About window.
