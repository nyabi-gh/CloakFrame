"""OpenCV reference data for the ORT spike on Linux and Windows (x86, no KleidiCV HAL).

Usage: python opencv_reference.py <out dir> <image>...

Writes <out>/decoded/<stem>.png (OpenCV decode, the pixels 1.12.1 feeds its detectors) with
resized_640_*/resized_512_* next to them, and <out>/resize-ipp-on|off/ with 36 size pairs for
`ort-spike resize-check`. Needs opencv-python-headless==5.0.0.93 and numpy.
"""

import json
import sys
from pathlib import Path

import cv2
import numpy as np

SOURCES = [
    (1, 1), (3, 2), (8, 8), (15, 9), (64, 48), (333, 222),
    (512, 512), (640, 427), (1280, 853), (1920, 1080), (3000, 2000), (4032, 3024),
]


def fit(w, h, target, rounded):
    scale = min(target / w, target / h)
    if rounded:
        return max(1, round(w * scale)), max(1, round(h * scale))
    return max(1, int(w * scale)), max(1, int(h * scale))


def main():
    out = Path(sys.argv[1])
    decoded = out / "decoded"
    decoded.mkdir(parents=True, exist_ok=True)
    print("OpenCV", cv2.__version__, "IPP", cv2.ipp.useIPP())
    for name in sys.argv[2:]:
        bgr = cv2.imread(name, cv2.IMREAD_COLOR)
        if bgr is None:
            raise SystemExit(f"cannot read {name}")
        stem = Path(name).stem
        cv2.imwrite(str(decoded / f"{stem}.png"), bgr)
        h, w = bgr.shape[:2]
        for target, rounded in ((640, False), (512, True)):
            tw, th = fit(w, h, target, rounded)
            cv2.imwrite(str(decoded / f"resized_{target}_{stem}.png"),
                        cv2.resize(bgr, (tw, th), interpolation=cv2.INTER_LINEAR))

    rng = np.random.default_rng(20261016)
    for ipp in (True, False):
        cv2.ipp.setUseIPP(ipp)
        folder = out / f"resize-ipp-{'on' if ipp else 'off'}"
        folder.mkdir(parents=True, exist_ok=True)
        index = []
        for i, (w, h) in enumerate(SOURCES):
            src = rng.integers(0, 256, (h, w, 3), dtype=np.uint8)
            cv2.imwrite(str(folder / f"src_{i}.png"), src)
            targets = [fit(w, h, 640, False), fit(w, h, 512, True), (max(1, w // 2 + 1), max(1, h // 3 + 1))]
            for tw, th in targets:
                ref = f"ref_{i}_{tw}x{th}.png"
                cv2.imwrite(str(folder / ref), cv2.resize(src, (tw, th), interpolation=cv2.INTER_LINEAR))
                index.append({"src": f"src_{i}.png", "target": [tw, th], "ref": ref})
        (folder / "index.json").write_text(json.dumps(index, indent=1))
        print(folder, len(index), "pairs")


if __name__ == "__main__":
    main()
