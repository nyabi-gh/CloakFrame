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
    [0] Today
    [1] Yesterday
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
session-back = Back
session-item-changed = The original changed or is gone; run it again
discard-title = Discard this review?
discard-body = The detections kept for the review are deleted. The originals and the saved files stay as they are.
discard-confirm = Discard
discard-keep = Keep
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

run-review = Review Now
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
editor-to-queue = All Items
editor-previous = Previous
editor-next = Next
editor-position = { $at } of { $count }
editor-failed = Not saved: { $error }
editor-detections = { $count ->
    [one] { $masked } of one detection masked
   *[other] { $masked } of { $count } detections masked
}
editor-added = { $count ->
    [one] One area added
   *[other] { $count } areas added
}
editor-problems = Problems
editor-no-problems = No problems found.
editor-problem-open = Not resolved
editor-problem-covered = Covered by a mask
editor-problem-dismissed = Nothing to mask
editor-cover = Mask It
editor-dismiss = Nothing to Mask
editor-reopen = Reopen
editor-undo = Undo
editor-redo = Redo
editor-shortcuts = Shortcuts (?)
editor-approve = Approve and Save
editor-approve-blocked = Resolve every problem first, or export with warnings.
editor-export-warnings = Export with Warnings…
editor-more = More
editor-export-plain = Export Without Anonymization…
editor-video-loading = Opening the video…
editor-video-unreadable = The video cannot be shown: { $error }
editor-video-canvas = Video editor
editor-video-decoding = Decoding the frame…
editor-video-frame-failed = The frame cannot be shown: { $error }
editor-video-frame = Frame { $frame } of 0–{ $last } · { $time }
editor-video-play = Play
editor-video-stop = Stop
editor-video-masked = Play masked
editor-timeline = Timeline
editor-tracks = { $count ->
    [one] One track, { $masked } masked
   *[other] { $count } tracks, { $masked } masked
}
editor-drawn = { $count ->
    [one] One drawn track
   *[other] { $count } drawn tracks
}
editor-track-face = Face, frames { $first }–{ $last }
editor-track-plate = License plate, frames { $first }–{ $last }
editor-track-low = Low confidence: masked, but check it is a face or plate.
editor-track-masked = Masked
editor-track-unmasked = Not masked
editor-track-mask = Mask This Track
editor-track-unmask = Do Not Mask This Track
editor-drawn-track = Drawn track, frames { $first }–{ $last }, { $keyframes ->
    [one] one keyframe
   *[other] { $keyframes } keyframes
}
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
refused-other = The decision was refused: { $reason }
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
