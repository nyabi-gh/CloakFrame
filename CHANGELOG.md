# Changelog

What changed in each CloakFrame release, written for the people using it. The release workflow publishes a version's section as the notes of its GitHub release. Releases before 1.12.0 list their changes on their release pages.

## 1.12.1

### Fixed

- **Linux:** the AppImage starts again. Every AppImage from 1.11.0 to 1.12.0 failed to start. It now runs on Ubuntu 24.04, Debian 13, Fedora 43, Arch and other distributions with glibc 2.39 or later, and it reads TIFF and WebP photos on distributions that no longer ship the old libtiff.
- **Photo review:** Return no longer removes a box you have just added. It now toggles only detected boxes; remove an added box with Delete or by clicking it.
- **Video review:** Encode video now asks whether to place a dashed box that is still waiting to be placed, instead of encoding without it.
- **Video review:** when a gap at a scene cut was covered, unchecking the track before the cut also uncovered the frames after it. Each track now covers only its own side of the cut.

### Changed

- Windows and Linux builds use Qt 6.11.2, like the macOS build.

### Notes

- If you are on 1.11.3 or earlier and the app does not offer this update, download it from this page. Only 1.12.0 and later find new versions on their own.

## 1.12.0

### Highlights

- **macOS:** videos can be processed again. Since 1.11.0 the app refused its own bundled FFmpeg.
- **Audio:** a new **Remove audio from videos** option. Masking covers only the picture; when videos keep their audio, the results now say that voices are not anonymized.
- **Metadata:** **Preserve selected EXIF metadata** now keeps only camera, lens, orientation, capture time and exposure details. Location is kept only with the new **Also keep location (GPS)** option, which is off. Serial numbers and owner or author names are always removed.
- **Keyboard:** missed regions can be added and adjusted from the keyboard in photo and video review.

### Video tracking

- Short gaps at scene cuts are covered like gaps inside a track instead of being reported, and each gap is counted once, in frames of the video. On a test clip, warnings went from 33 to 4.
- Faces the detector missed for a few frames right after a scene cut are reported as tracking gaps instead of being left unmasked without a warning.
- With review off, a track with at least one confident detection is masked in every frame, not only in its confident ones.

### Review

- Video review asks before leaving: **Skip video** moves on to the next file, Esc and closing the window ask whether to skip this video, and **Cancel all** asks before stopping the run. Esc while drawing only stops drawing.
- Return never confirms a review dialog, and in File results it no longer opens the unredacted original.
- Retrying a file from File results no longer changes the main window's inputs, output folder or review setting, and the other files can still be reviewed afterwards.
- After clicking Undo or Redo, the keyboard stays on the photo.
- The video frame and the timeline have names for screen readers; placeholder text and the light theme's timeline marks have more contrast; an excluded track is drawn dotted instead of only in gray.

### Results and progress

- A run without warnings ends with "Done — no reported warnings", and says "Not reviewed" when review was off.
- Files of unsupported types, such as HEIC or MKV, are counted in the log instead of being skipped silently.
- The progress bar moves while a video is analyzed and encoded.
- A missing FFmpeg is reported once per run.
- A failed model download says why, and gives the link and the path to install the model by hand.
- Checking model files no longer freezes the window.

### Masking

- Ellipse masks always cover the whole detected box, and regions thinner than 2 pixels are filled instead of being left unchanged.
- Soft-edge masks are computed in parallel, which speeds up runs with soft edges on.
- If GPU acceleration fails in the middle of a run, detection continues on the CPU from the same file.

### Privacy and security

- Crash dumps are turned off, so they cannot capture the photos and videos being processed.
- FFmpeg opens inputs only as local files and never reaches the network.
- Files left in the output folder by a crashed run are cleaned up at the next start, and output folder paths are no longer kept after a run ends.
- Folder scans no longer follow NTFS junctions out of the chosen folder.
- A custom model is approved by the same digest the detector loads.
- An update that fails signature verification is refused, instead of the app pointing you to its download page.
- Update signatures now cover the version and file name, not only the package contents.

### Platforms and packaging

- Windows and Linux read TIFF and WebP photos.
- The Linux AppImage bundles FFmpeg. (This release's AppImage does not start; see 1.12.1.)
- Each release offers three downloads, named by version and platform, with their SHA-256 in the release notes.
- The app no longer shows release notes; the update prompt names the version.
- The Simplified Chinese translation was removed.
