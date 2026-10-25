# The source text. ko.ftl and ja.ftl translate these ids; an id missing there falls back to this.

app-name = CloakFrame

drop-hint = Drop images, videos, or folders here
drop-release = Release to add { $count ->
    [one] one item
   *[other] { $count } items
}
add-files = Add Files…
add-folder = Add Folder…
clear-list = Clear
scanning = Looking for photos and videos…
found-summary = { $photos ->
    [one] one photo
   *[other] { $photos } photos
}, { $videos ->
    [one] one video
   *[other] { $videos } videos
}
skipped-type = { $count ->
    [one] one file
   *[other] { $count } files
} of type { $extension } cannot be processed
skipped-no-extension = { $count ->
    [one] one file
   *[other] { $count } files
} without an extension cannot be processed
unreadable = Cannot read { $path }

what = What to hide
faces = Faces
plates = License plates
how = How
method-fill = Fill
method-mosaic = Mosaic
method-blur = Blur
strength = Strength
models-need-one = Choose faces, license plates, or both.

output-folder = Output folder
choose-folder = Choose…
no-output-folder = No folder chosen
start = Start
start-needs-files = Add photos or videos first.
start-needs-output = Choose an output folder first.

menu-app-about = About CloakFrame
menu-app-quit = Quit CloakFrame
menu-file = File
menu-file-quit = Exit
menu-edit = Edit
menu-edit-undo = Undo
menu-edit-redo = Redo
menu-edit-cut = Cut
menu-edit-copy = Copy
menu-edit-paste = Paste
menu-edit-select-all = Select All
menu-view = View
menu-help = Help

theme = Theme
theme-system = System
theme-light = Light
theme-dark = Dark
language = Language
language-system = System

about-title = About CloakFrame
about-version = Version { $version }
about-license = CloakFrame is free software under the Apache License 2.0.
about-third-party = Third-party notices
about-models = Detection models
close = Close

settings-unknown-keys = Settings this version does not know were kept: { $keys }
settings-unreadable = The settings file could not be read and was left as it is: { $error }
settings-newer = A newer version of CloakFrame wrote the settings (format { $version }); changes here are not saved.

review-first = Review before saving
running = Working…
cancel = Cancel
state-queued = Waiting
state-analyzed = Analyzed
state-needs-review = Needs review
state-rendering = Saving
state-verified = Checked
state-published = Saved
state-failed = Failed
state-cancelled = Cancelled
run-summary = { $saved ->
    [one] One saved
   *[other] { $saved } saved
}, { $review } to review, { $failed } failed
run-new = New run
start-needs-runtime = ONNX Runtime was not found next to the app.
model-missing = The detection model { $name } is not downloaded yet.
videos-need-ffmpeg = FFmpeg was not found next to the app, so videos are left out.

models-title = Detection models
models-open = Detection Models…
models-folder = Folder: { $path }
model-name-yolo5face-n = YOLO5Face-n (faces, recommended)
model-name-yunet = YuNet (faces, fast)
model-name-plates = YOLOv9-t (license plates)
model-terms-yolo5face-n = Converted from YOLO5Face (GPL-3.0) and trained on WIDER FACE, a dataset for non-commercial research: use it for non-commercial purposes only.
model-terms-yunet = From the OpenCV Zoo under the MIT license.
model-terms-plates = From the open-image-models project under the MIT license; the terms of YOLOv9, which it builds on, also apply.
model-size = { $size } MB
model-license = License: { $license }
model-present = Downloaded
model-not-downloaded = Not downloaded
model-different = The file in its place is not this model; downloading replaces it.
model-accept-terms = I accept these terms
model-download = Download
model-download-failed = The download failed: { $error }
