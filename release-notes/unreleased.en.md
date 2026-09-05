## Result reliability

- Video review now lists tracking gaps chronologically with millisecond times
  and frame ranges, plus previous/next gap navigation.
- Move one frame at a time with Left/Right arrow keys or timeline buttons.
  Changing frames cancels an in-progress box drag so it cannot affect another frame.

- After a run, use **File results…** to filter attention items or failures,
  inspect details, and open input or output folders. The output folder action
  is available only for published results. Results reset on the next run.
- Retain results from already running files when parallel processing is cancelled.

- Report inaccessible subfolders with their exact paths and continue scanning
  other readable folders. Directory symlinks are not followed recursively.
- Correct the video review instructions: low-confidence tracks are included
  by default, matching their actual selection state.
- Show omitted detection regions, tracking gap frames, dropped tracks, files
  with metadata warnings, and unreadable input paths separately in the summary.
- Clarify that red timeline marks and tracking warnings describe gaps found
  before review. Adding a manual mask does not verify the subject's actual
  position, so these warnings remain. Check those ranges before sharing.
