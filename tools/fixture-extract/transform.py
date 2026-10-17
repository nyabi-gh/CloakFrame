"""Rewrite an exported copy of the 1.12.1 tests so they record fixtures through record.hpp.

Usage: transform.py <exported tests dir>

Only the exported copy is changed; the 1.12.1 sources in the repository stay as they are.
"""

import re
import sys
from pathlib import Path

RECORDED = [
    "buildTracks",
    "buildBidirectionalTracks",
    "interpolateGaps",
    "smoothTrack",
    "extendTrackEnds",
    "postProcessTracks",
    "applyTrackingWindows",
    "manualTrackRectAtFrame",
    "materializeManualVideoTrack",
    "applyAnonymization",
    "applyMosaic",
    "applyOrientation",
    "nonMaxSuppression",
    "intersectionOverUnion",
    "encodeParamsForExtension",
    "readExifOrientation",
    "sniffImageFormat",
    "imageFrameCount",
    "copyMetadata",
    "crfForQuality",
    "videoUnsupportedReason",
    "probeVideo",
    "isSupportedVideo",
    "videoStrongScoreThreshold",
    "videoMaskingPlan",
    "verifyUpdateSignature",
    "updateSignaturePayload",
    "evaluateUpdateTrust",
]


def transform(text: str) -> str:
    text = re.sub(r"(?<![\w])assert\(", "CF_ASSERT(", text)
    for name in RECORDED:
        text = re.sub(rf"\bcloakframe::{name}\(", f"cfrec::{name}(", text)
    text = re.sub(r"\bcloakframe::SceneCutDetector(\s+\w+\s*;)", r"cfrec::SceneCutDetector\1", text)

    main = re.search(r"^int main\(.*?^}", text, flags=re.S | re.M)
    if main is None:
        raise SystemExit("no main() found")
    body, calls = re.subn(
        r"^(\s+)(test\w+)\((.*)\);",
        r'\1cfrec::begin("\2");\n\1\2(\3);',
        main.group(0),
        flags=re.M,
    )
    if calls == 0:
        # Suites that assert directly in main() become one fixture named after the suite.
        body = body.replace("{", '{\n    cfrec::begin("main");', 1)
    text = text[: main.start()] + body + text[main.end():]
    return '#include "record.hpp"\n' + text


def main() -> None:
    tests = Path(sys.argv[1])
    for source in sorted(tests.glob("test_*.cpp")):
        if source.name == "test_support.cpp":
            continue
        source.write_text(transform(source.read_text()))


if __name__ == "__main__":
    main()
