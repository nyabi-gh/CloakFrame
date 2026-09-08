## Review, results, and responsiveness

- Mark each tracking gap as reviewed and see pending frame counts separately. Editing masks resets these checks. Acknowledgement records user review, not verified coverage.
- Filter file results by issue type, open an input, or review again starting at a selected gap. Reprocessing uses current settings and a new output folder; previous edits are not retained.
- Image and video thumbnails load in the background, with at most two active jobs.
- Basic local logs omit file names. Detailed logging requires a separate opt-in and changes take effect immediately. Settings now offer log-folder access and log deletion.
- Model downloads have a 30-second inactivity limit, a 15-minute overall limit, and up to three attempts. Cancellation, timeout, network, and integrity failures are distinguished.

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
