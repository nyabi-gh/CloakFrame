# The source text. ko.ftl and ja.ftl translate these ids; an id missing there falls back to this.

app-name = CloakFrame

drop-hint = Drop images, videos, or folders here
drop-release = Release to add { $count ->
    [one] one item
   *[other] { $count } items
}
add-files = Add Files…
add-folder = Add Folder…
clear-list = Remove All
scanning = Looking for photos and videos…
found-summary = { $photos ->
    [one] one photo
   *[other] { $photos } photos
}, { $videos ->
    [one] one video
   *[other] { $videos } videos
}
unreadable = Cannot read { $path }

what = What to hide
faces = Faces
plates = License plates
how = How
method-fill = Fill
method-mosaic = Mosaic
method-blur = Blur
strength = Strength
shape = Shape
shape-rectangle = Rectangle
shape-ellipse = Ellipse
models-need-one = Choose faces, license plates, or both.

output-folder = Output folder
choose-folder = Choose…
no-output-folder = No folder chosen
start = Start
start-needs-files = Add photos or videos first.
start-scanning = Wait until the photos and videos are found.
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
about-graphics = Graphics: { $graphics }
about-third-party = Third-party notices
about-models = Detection models
close = Close

settings-unknown-keys = Settings this version does not know were kept: { $keys }
settings-unreadable = The settings file could not be read and was left as it is: { $error }
settings-newer = A newer version of CloakFrame wrote the settings (format { $version }); changes here are not saved.

review-first = Review before saving
running = Processing
cancel = Cancel
state-queued = Waiting
state-analyzing = Analyzing
state-analyzed = Analyzed
state-needs-review = Needs review
state-rendering = Saving
state-verified = Checked
state-published = Saved
state-failed = Failed
state-cancelled = Cancelled
state-cancelling = Cancelling
summary-saved = { $count } saved
summary-review = { $count } to review
summary-failed = { $count } failed
summary-cancelled = { $count } cancelled
summary-none = Nothing was saved
summary-with-details = { $counts } — { $details }
summary-missed-faces = faces missed for { $seconds } s in { $videos ->
    [one] one video
   *[other] { $videos } videos
}
summary-missed-plates = license plates missed for { $seconds } s in { $videos ->
    [one] one video
   *[other] { $videos } videos
}
summary-capped = the detection limit reached in { $count ->
    [one] one item
   *[other] { $count } items
}
summary-weak = { $count ->
    [one] an uncertain detection in one item
   *[other] uncertain detections in { $count } items
}
summary-failed-frames = frames that could not be analyzed in { $count ->
    [one] one video
   *[other] { $count } videos
}
list-separator = {", "}
problem-missed-face = Face lost in frames { $first }–{ $last } ({ $seconds } s)
problem-missed-plate = License plate lost in frames { $first }–{ $last } ({ $seconds } s)
problem-capped = Detection limit reached: { $kept } of { $found } kept
problem-failed-frame = Frame { $frame } could not be analyzed
problem-weak = Uncertain detection (score { $score })
progress-percent = { $percent }%
progress-left-estimating = estimating the time left
progress-left-under-minute = less than a minute left
progress-left-minutes = about { $minutes ->
    [one] one minute
   *[other] { $minutes } minutes
} left
progress-left-hours = about { $hours } h { $minutes } min left
run-session-kept = The items to review are kept, and stay listed after the app restarts.
item-reveal = Show in Folder
item-open = Open
masked-nothing = Nothing masked
masked-faces = { $faces ->
    [one] 1 face
   *[other] { $faces } faces
}
masked-plates = { $plates ->
    [one] 1 license plate
   *[other] { $plates } license plates
}
masked-both = { $faces ->
    [one] 1 face
   *[other] { $faces } faces
} · { $plates ->
    [one] 1 license plate
   *[other] { $plates } license plates
}
item-reveal-source = Show Original
shell-failed = Cannot show { $path }: { $error }
log-title = Details
log-copy = Copy
quit-title = A run is in progress
quit-body = Quitting cancels it. Saved files stay, and the items waiting for review are kept for later.
quit-confirm = Cancel and Quit
quit-keep = Keep Working
quit-waiting = Stopping; the window closes once the run has stopped.

sessions-title = Waiting for review
session-age = { $days ->
    [0] Today { $time }
    [1] Yesterday { $time }
   *[other] { $days } days ago
}
session-row = { $items ->
    [one] one item
   *[other] { $items } items
} for { $output }
session-changed = { $count ->
    [one] one original changed
   *[other] { $count } originals changed
}
session-unreadable = Cannot read it: { $error }
session-open = Open
session-discard = Discard…
session-item-changed = The original changed or is gone; run it again
discard-title = Discard this review?
discard-body = The detections kept for the review are deleted. The originals and the saved files stay as they are.
discard-confirm = Discard
discard-keep = Keep
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

item-review = Review
start-during-review = Close the review first.
review-in-use = Another run or review has this session open.
review-done = Every item of this review is saved.
review-options-guessed = The app did not make this session, so it is saved with the current settings; items analyzed with other settings are refused.
review-badge-changed = Original changed
review-badge-saving = Saving…
review-badge-again = Decide again
review-badge-problems = { $count ->
    [one] One problem
   *[other] { $count } problems
}
review-badge-nothing = Nothing detected
review-badge-waiting = To review
editor-canvas = Photo editor
editor-previous = Previous
editor-next = Next
editor-position = { $at } of { $count }
editor-failed = Not saved: { $error }
editor-detections = { $count ->
    [one] { $masked } of one detection masked
   *[other] { $masked } of { $count } detections masked
}
editor-problems = To resolve
editor-no-problems = No problems found.
editor-problem-open = Not resolved
editor-problem-covered = Covered by a mask
editor-problem-dismissed = Nothing to mask
editor-cover = Mask It
editor-dismiss = Nothing to Mask
editor-reopen = Reopen
editor-undo = Undo
editor-redo = Redo
editor-shortcuts = ? Shortcuts
editor-approve = Approve and Save
editor-approve-blocked = Resolve every problem first, or save with warnings from the More menu.
editor-export-warnings = Save with Warnings…
editor-more = More
editor-export-plain = Export Without Anonymization…
review-left = { $count ->
    [0] Nothing left to review
    [one] 1 left to review
   *[other] { $count } left to review
}
review-discard = Discard This Review…
review-kept = Saved items
editor-fit = Fit
editor-solve-first = Resolve the problems to save
editor-added-title = Areas you added
editor-added-area = Area { $n }
editor-remove = Remove
editor-hint = Click to mask or unmask · Drag to add an area · Hold Space to see the result
editor-kind-face = Face
editor-kind-plate = License plate
editor-kind-drawn = Drawn
editor-track-weak = Not sure
editor-video-total = / { $total } · frame { $frame }
editor-video-loading = Opening the video…
editor-video-unreadable = The video cannot be shown: { $error }
editor-video-canvas = Video editor
editor-video-decoding = Decoding the frame…
editor-video-frame-failed = The frame cannot be shown: { $error }
editor-video-play = Play
editor-video-stop = Stop
editor-video-masked = Show masked
editor-timeline = Timeline
editor-tracks = { $count ->
    [one] One track, { $masked } masked
   *[other] { $count } tracks, { $masked } masked
}
editor-track-low = Low confidence: masked, but check it is a face or plate.
editor-drawn-start = Start Here
editor-drawn-end = End Here
editor-drawn-remove-keyframe = Remove This Keyframe
editor-drawn-remove = Remove Track
editor-cover-gap = Draw a Track Over It
editor-loading = Opening the photo…
editor-unreadable = The photo cannot be shown: { $error }
editor-preview = Masking preview
refused-settings = This item was analyzed with other settings; run it again.
refused-stale = This item was decided elsewhere meanwhile; open the review again.
refused-name = The output folder already has a file of this name. A free name: { $name }
refused-rename = Save as { $name }
refused-invalid-name = The output name is not a valid file name.
refused-unresolved = This item still has unresolved problems.
refused-not-covered = An added area no longer covers a problem marked as covered by it.
refused-internal = The decision was refused because of a fault in CloakFrame: { $reason }
confirm-warnings-title = Export with unresolved problems?
confirm-warnings-body = { $count ->
    [one] One problem is
   *[other] { $count } problems are
} not resolved, so a face or license plate may be left unmasked. The item is not counted as finished normally.
confirm-warnings = Export with Warnings
confirm-plain-title = Export without anonymization?
confirm-plain-body = Nothing is masked: the item is saved again under the metadata rules only. The item is not counted as finished normally.
confirm-plain = Export Without Anonymization
confirm-cancel = Cancel
shortcuts-title = Shortcuts
shortcuts-close = Close
keys-click = Click
keys-drag = Drag
keys-arrows = Arrow keys
keys-alt-arrows = Alt + arrow keys
keys-space = Space (hold)
keys-m = M (hold)
keys-alt-command-arrows = Alt + { $command } + arrow keys
video-shortcut-click = Mask or unmask a track; select a drawn track
video-shortcut-drag = Draw a track from this frame for a second; drag a drawn box to set it at this frame
video-shortcut-space = Play or stop
video-shortcut-mask = Show this frame masked
video-shortcut-frames = Previous or next frame (Shift: one second)
video-shortcut-ends = First or last frame
video-shortcut-gaps = Previous or next gap
video-shortcut-enter = Mask or unmask the selected track
video-shortcut-alt-arrows = Move the selected drawn box at this frame (Shift: 10 pixels)
video-shortcut-resize = Resize the selected drawn box at this frame
video-shortcut-delete = Remove the selected drawn track
shortcut-click = Mask or unmask a detection; select an added area
shortcut-drag = Add an area to mask; drag an added area to move it
shortcut-tab = Select the next or previous box
shortcut-enter = Mask or unmask the selected detection
shortcut-arrows = Move the selected area (Shift: 10 pixels); move the view
shortcut-alt-arrows = Resize the selected area
shortcut-delete = Remove the selected area
shortcut-space = Preview the masking
shortcut-undo = Undo
shortcut-redo = Redo
shortcut-zoom = Zoom in, zoom out, fit, full resolution
shortcut-pages = Previous or next item
shortcut-help = Show or hide this list
shortcut-scroll = Scroll to move the view; pinch or Ctrl + scroll to zoom.

editor-cancelled = Not saved: cancelled

run-failed = The run could not start: { $error }
run-failed-no-data-folder = There is no app data folder for this user.

failure-unsupported = This file's format or content is not one CloakFrame processes.
failure-too-large = This file is larger than CloakFrame processes at once.
failure-unreadable = The file cannot be read; it may be damaged.
failure-source-changed = The file changed or went away while it was processed; run it again.
failure-name-taken = A file of the output's name appeared during the run, and was not replaced.
failure-output = The output cannot be written to the output folder.
failure-encode = The output could not be encoded.
failure-detection = Detection failed on this file.
failure-tool = FFmpeg could not be started.
failure-other = Something went wrong with this file.

note-color-converted = Colors converted to sRGB
note-color-profile-ignored = An unusable color profile was dropped; the colors were read as sRGB
note-nothing-detected = Nothing was detected
note-tracked-nothing = Nothing was tracked
note-tracked-faces = Tracked { $faces ->
    [one] one face
   *[other] { $faces } faces
}
note-tracked-plates = Tracked { $plates ->
    [one] one license plate
   *[other] { $plates } license plates
}
note-tracked-both = Tracked { $faces ->
    [one] one face
   *[other] { $faces } faces
} and { $plates ->
    [one] one license plate
   *[other] { $plates } license plates
}
note-analyzed-again-on-cpu = The GPU failed, so this item was analyzed again on the CPU, as is the rest of the run
note-encoded-again-in-software = The hardware encoder { $encoder } failed, so this video was encoded again in software, as is the rest of the run: { $reason }
note-padded = The odd size was padded to { $width }×{ $height }
note-folder-not-flushed = Saved, but the output folder could not be flushed to disk
note-temp-file-left = Saved, but a temporary file was left next to it; a later run removes it
note-records-left = Saved, but the video's analysis records were left in the app's data folder; a later run removes them

run-note-not-swept = Cannot remove { $path }: { $error }
run-note-model-copied = Copied { $name } from CloakFrame 1.12.1 ({ $from })
run-note-legacy-coreml = Cannot remove the models CloakFrame 1.12.1 compiled for CoreML: { $error }
run-note-unused-coreml = Cannot remove compiled CoreML models this version does not use: { $error }
run-note-no-estimate = Cannot estimate the work: { $error }
run-note-session-not-saved = Cannot save the review session: { $error }
run-note-records-left = Cannot remove the video analysis records in { $path }: { $error }
run-note-left-out-not-analyzed = { $name } cannot wait for review: the run kept no record of what it analyzed.
run-note-left-out-changed = { $name } cannot wait for review: it changed or went away after it was analyzed.

advanced = Advanced Options
face-model = Face model
face-model-accurate = Accurate  ·  YOLO5Face-n
face-model-fast = Fast  ·  YuNet
sensitivity = Sensitivity
sensitivity-lowest = Lowest
sensitivity-low = Low
sensitivity-default = Default
sensitivity-high = High
sensitivity-highest = Highest
sensitivity-hint = Higher finds more faces and license plates but masks more that are not; lower misses more. Masks from score { $score }.
tiles = Look for small faces in tiles
tiles-hint = Also detects in overlapping parts of large photos and videos, for faces too small to find in the whole picture. Slower.
keep-metadata = Preserve selected EXIF metadata
keep-metadata-hint = Off (default): saved photos carry no metadata. On: only the camera, lens, exposure and capture time are copied, and the location only if you also choose it. Videos never keep metadata.
keep-gps = Also keep location (GPS)
remove-audio = Remove audio from videos
remove-audio-hint = Off (default): videos keep their audio. Voices and anything said, such as names or phone numbers, are not anonymized.
video-quality = Video quality
video-quality-high = High (near-original)
video-quality-balanced = Balanced
video-quality-space-saver = Smaller files
video-quality-hint = Quality of re-encoded videos. Higher quality produces larger files.
video-codec = Video codec
video-codec-h264 = H.264 (most compatible)
video-codec-hevc = HEVC (smaller files)
video-codec-hint = Codec for re-encoded videos. HEVC produces smaller files but may not play on older devices.
gpu = Use GPU acceleration
gpu-hint = Runs detection models and video encoding on the GPU when available. Applies from the next run.
advanced-next-run = These apply from the next run; a review saves with the settings its run used.
sample-title = Preview
sample-alt = Anonymization style preview

found-list = Photos and videos to process
found-photo = Photo { $name }
found-video = Video { $name }
run-list = Items of this run
review-queue = Items to review

graphics-failed-title = CloakFrame cannot start
graphics-failed =
    CloakFrame could not draw its window with the graphics this computer offers. Updating the graphics driver may help.

    { $errors }

new-task = New Task
add = Add…
add-menu-files = Files…
add-menu-folder = Folder…
drop-or = or
choose-files = Choose Files…
found-counts = { $photos ->
    [one] one photo
   *[other] { $photos } photos
} · { $videos ->
    [one] one video
   *[other] { $videos } videos
}
skipped-summary = { $count ->
    [one] One file is
   *[other] { $count } files are
} not processed — { $parts }
skipped-part-type = { $count } { $extension } (unsupported type)
skipped-part-no-extension = { $count } without an extension
skipped-part-unreadable = { $count } unreadable
change-folder = Change…
advanced-open = Advanced Options…
advanced-detection = Detection
advanced-photos = Photos
advanced-videos = Videos
done = Done
session-unreadable-short = Cannot be read
result-title = Done
cancel-all = Cancel All
cancel-item = Cancel This Item
review-all = Start Review
run-ended = { $when } · { $folder }
