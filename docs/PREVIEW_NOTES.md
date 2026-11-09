A preview of CloakFrame 2, rewritten in Rust, for macOS on Apple silicon (macOS 15 or later), Windows 10 version 1903 or later (x64) and Linux x86_64 with glibc 2.39 or later. It is not the stable release. It masks photos and videos (MP4, MOV, M4V and WebM in H.264, HEVC, VP8 or VP9, 8-bit SDR). Video support is new in this preview: play each result to the end before sharing it.

**macOS:** unzip, move "CloakFrame Preview" to Applications and open it. The app is not notarized, so macOS blocks its first open; open System Settings > Privacy & Security and choose "Open Anyway". The macOS preview does not update itself.

**Windows:** run the Setup file. It is not code signed, so SmartScreen may warn on its first run; choose "More info", then "Run anyway". It installs for your account only, as "CloakFrame 2", and later previews come to it as updates, each checked against CloakFrame's own signature before it is installed. It includes Microsoft's Windows ML runtime, which is licensed under Microsoft's terms in THIRD_PARTY_NOTICES.txt (also in the About window); installing or using the app means agreeing to them for that runtime.

**Linux:** make the AppImage executable (`chmod +x CloakFrame-*-Linux-x86_64.AppImage`) and run it. It needs glibc 2.39 or later, such as Ubuntu 24.04, Debian 13 or Fedora 43, and FUSE's `fusermount`, which most desktops have; without FUSE, set `APPIMAGE_EXTRACT_AND_RUN=1`. Later previews replace the file as updates, each checked against CloakFrame's own signature before it is installed. Detection runs on the CPU unless you download the NVIDIA GPU pack in Settings.

**Coming from 1.x:** CloakFrame 1.x does not update to 2, so install the preview as a new app. It sits next to CloakFrame 1.12.1, which keeps its settings and keeps working. The preview starts with its own default settings, since settings from 1.12.1 are not carried over, and uses the detection models 1.12.1 already downloaded.

**Since preview.2:** videos are masked. The preview includes ffmpeg and ffprobe, built for CloakFrame from FFmpeg 9.0.2 with x264 and x265 and only what CloakFrame reads; they are licensed under the GNU General Public License, version 2 or later, and their corresponding source is CloakFrame-FFmpeg-9.0.2-1-source.tar in this release. A video keeps its sound unless Remove audio from videos is on in Settings; one whose sound is in a rare old format, such as QDesign or QCELP, stops before it is analyzed and says to remove the audio. Encoding uses VideoToolbox on macOS and NVIDIA or Intel hardware on Windows where it works, and the CPU otherwise.

**Since preview.1:** the panel has a Coverage row (narrow, standard or wide) that sets how far each mask reaches past the detected face or plate, and the CLI takes `--coverage`. Until an output folder is picked, results go to CloakFrame in the Pictures folder, as in 1.12.1, so Start no longer waits for a folder. The controls, buttons and alignment were redrawn to one design, and Settings opens as a dialog in the middle of the window.

**Detection models** are downloaded from the Detection models tab of Settings, each after its terms are accepted.

What changed from 1.12.1 is listed in [CHANGES_FROM_1.12.md](https://github.com/nyabi-gh/CloakFrame/blob/main/docs/CHANGES_FROM_1.12.md). Please report problems as issues, with the graphics line from the About window.
