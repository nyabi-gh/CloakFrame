"""OpenCV cv::resize(INTER_LINEAR) reference hashes for the engine's resize port.

Usage: python -I make.py <out json>

Needs opencv-python-headless==5.0.0.93 and numpy on x86 (no KleidiCV HAL, docs/M0_ORT_SPIKE.md).
Sources are the noise pattern below, which the Rust test makes the same way, so only the SHA-256
of each OpenCV output is stored. Exact 2x reductions in both directions are where OpenCV switches
INTER_LINEAR to its INTER_AREA fast path.
"""

import hashlib
import json
import platform
import sys
from pathlib import Path

import cv2
import numpy as np

SOURCES = [
    (1, 1), (2, 1), (2, 2), (3, 2), (4, 4), (6, 2), (8, 8), (15, 9), (16, 10), (34, 6),
    (64, 48), (66, 4), (333, 222), (512, 512), (548, 342), (640, 427), (1280, 853),
    (1280, 1280), (1920, 1080),
]


def pattern(w, h):
    y, x, c = np.meshgrid(np.arange(h, dtype=np.uint32), np.arange(w, dtype=np.uint32),
                          np.arange(3, dtype=np.uint32), indexing="ij")
    v = x * np.uint32(0x9E3779B1) + y * np.uint32(0x85EBCA77) + c * np.uint32(0xC2B2AE3D)
    v ^= v >> np.uint32(15)
    v *= np.uint32(0x2C1B3C6D)
    v ^= v >> np.uint32(12)
    return (v >> np.uint32(24)).astype(np.uint8)


def fit(w, h, target, rounded):
    scale = min(target / w, target / h)
    if rounded:
        return max(1, round(w * scale)), max(1, round(h * scale))
    return max(1, int(w * scale)), max(1, int(h * scale))


def targets(w, h):
    out = [fit(w, h, 640, False), fit(w, h, 512, True), (max(1, w // 2 + 1), max(1, h // 3 + 1))]
    if w % 2 == 0 and h % 2 == 0:
        out.append((w // 2, h // 2))
    if w % 2 == 0 and w > 2:
        out.append((w // 2, h))
    if w > 1 and h > 1:
        out.append((w // 2, h // 2) if (w % 2 or h % 2) else (w // 2 + 1, h // 2))
    return list(dict.fromkeys(out))


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    cases = []
    for w, h in SOURCES:
        src = pattern(w, h)
        for tw, th in targets(w, h):
            digests = set()
            for ipp in (True, False):
                cv2.ipp.setUseIPP(ipp)
                out = cv2.resize(src, (tw, th), interpolation=cv2.INTER_LINEAR)
                digests.add(hashlib.sha256(np.ascontiguousarray(out).tobytes()).hexdigest())
            if len(digests) != 1:
                raise SystemExit(f"IPP on and off differ for {w}x{h} -> {tw}x{th}")
            cases.append({"src": [w, h], "dst": [tw, th], "sha256": digests.pop()})
    doc = {
        "generator": "tools/resize-reference/make.py",
        "opencv": cv2.__version__,
        "machine": platform.machine(),
        "pattern": "v = x*0x9E3779B1 + y*0x85EBCA77 + c*0xC2B2AE3D; v ^= v>>15; v *= 0x2C1B3C6D;"
                   " v ^= v>>12; byte = v>>24 (u32 wrapping, HWC, 3 channels)",
        "cases": cases,
    }
    lines = [json.dumps(c, separators=(", ", ": ")) for c in doc.pop("cases")]
    head = json.dumps(doc, indent=1)[:-2]
    Path(sys.argv[1]).write_text(f'{head},\n "cases": [\n  ' + ",\n  ".join(lines) + "\n ]\n}\n",
                                 encoding="utf-8")
    print(len(cases), "cases")


if __name__ == "__main__":
    main()
