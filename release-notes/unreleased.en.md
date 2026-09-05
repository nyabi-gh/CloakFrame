## Result reliability

- Report inaccessible subfolders with their exact paths and continue scanning
  other readable folders. Directory symlinks are not followed recursively.
- Correct the video review instructions: low-confidence tracks are included
  by default, matching their actual selection state.
- Show omitted detection regions, tracking gap frames, dropped tracks, files
  with metadata warnings, and unreadable input paths separately in the summary.
- Clarify that red timeline marks and tracking warnings describe gaps found
  before review. Adding a manual mask does not verify the subject's actual
  position, so these warnings remain. Check those ranges before sharing.
