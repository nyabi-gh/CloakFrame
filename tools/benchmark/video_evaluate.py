"""Exposed face frames of video benchmark runs against the reference faces (docs/M5_BENCHMARK.md).

Usage: python -I tools/benchmark/video_evaluate.py [--gate] RUN...

Each RUN is a result of tools/benchmark/legacy/legacy_video.cpp or the engine's video_benchmark
example named <clip>.<label>.json, optionally gzipped; its reference is
tests/fixtures/benchmark/video/<clip>.reference.json. A reference face is exposed in its frame when
less than half its box lies under the run's masks: the track boxes of the frame grown by the
default padding and rounded outwards as the renderer does. Faces tagged on the sampled frames stand
for `sample_every` frames each, faces no run found (added on the audit frames) for `audit_every`.

--gate compares each label's total with tests/fixtures/benchmark/video/expected.json and fails
when it exposes more than one percent of the face frames above it, and fails when the engine at
1.12.1's setting (tiles-off) exposes more than 1.12.1 (legacy), plan 9.2.
"""

import gzip
import json
import math
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
FIXTURES = ROOT / "tests" / "fixtures" / "benchmark" / "video"
PADDING = 0.18
COVERED = 0.5
SIZES = [(0, 16), (16, 24), (24, None)]
GATE = 0.01


def load(path: Path):
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt", encoding="utf-8") as f:
        return json.load(f)


def padded(box, width, height):
    """1.12.1 `paddedRegion`, as the engine's `padded_region`."""
    x, y, w, h = box
    px, py = w * PADDING, h * PADDING
    left = math.floor(min(max(x - px, 0), width))
    top = math.floor(min(max(y - py, 0), height))
    right = math.ceil(min(max(x + w + px, 0), width))
    bottom = math.ceil(min(max(y + h + py, 0), height))
    left, top = min(left, width - 1), min(top, height - 1)
    return left, top, max(right, left + 1), max(bottom, top + 1)


def covered(box, masks, width, height):
    """The share of `box`'s whole pixels under the union of `masks`."""
    x, y, w, h = box
    left, top = max(0, math.floor(x)), max(0, math.floor(y))
    right, bottom = min(width, math.ceil(x + w)), min(height, math.ceil(y + h))
    if right <= left or bottom <= top:
        return 1.0
    rows = [[False] * (right - left) for _ in range(bottom - top)]
    for ml, mt, mr, mb in masks:
        for r in range(max(top, mt), min(bottom, mb)):
            row = rows[r - top]
            for c in range(max(left, ml), min(right, mr)):
                row[c - left] = True
    under = sum(sum(row) for row in rows)
    return under / ((right - left) * (bottom - top))


def size_name(lo, hi):
    return f"{lo}+ px" if hi is None else f"{lo}-{hi} px"


def evaluate(reference, run):
    """{(tag, size): [face frames, exposed face frames]} for one run."""
    width, height = reference["width"], reference["height"]
    if run["frames"] < reference["frames"]:
        raise SystemExit(f"the run has {run['frames']} frames, the clip {reference['frames']}")
    masks = {}
    for track in run["tracks"]:
        if track.get("kind", "face") != "face":
            continue
        for b in track["boxes"]:
            masks.setdefault(b[0], []).append(padded(b[1:5], width, height))
    counts = {}
    weighted = [(f, reference["sample_every"]) for f in reference["faces"]]
    weighted += [(f, reference["audit_every"]) for f in reference["added"]]
    for (frame, x, y, w, h, tag), weight in weighted:
        lo, hi = next(s for s in SIZES if h >= s[0] and (s[1] is None or h < s[1]))
        cell = counts.setdefault((tag, size_name(lo, hi)), [0, 0])
        cell[0] += weight
        if covered((x, y, w, h), masks.get(frame, []), width, height) < COVERED:
            cell[1] += weight
    return counts


def total(counts, tag="face"):
    frames = sum(v[0] for (t, _), v in counts.items() if t == tag)
    exposed = sum(v[1] for (t, _), v in counts.items() if t == tag)
    return frames, exposed


def main() -> None:
    args = sys.argv[1:]
    gate = "--gate" in args
    runs = [Path(a) for a in args if a != "--gate"]
    if not runs:
        raise SystemExit(__doc__)
    by_label = {}
    for path in runs:
        clip, label = path.name.split(".")[:2]
        reference = load(FIXTURES / f"{clip}.reference.json")
        counts = evaluate(reference, load(path))
        frames, exposed = total(counts)
        unsure = total(counts, "unsure")
        print(f"{clip} {label}: exposed {exposed} of {frames} face frames ({exposed / frames:.1%}),"
              f" unsure faces {unsure[1]} of {unsure[0]}")
        for lo, hi in SIZES:
            f, e = counts.get(("face", size_name(lo, hi)), [0, 0])
            if f:
                print(f"  {size_name(lo, hi):10} {e:6} of {f}")
        acc = by_label.setdefault(label, [0, 0])
        acc[0] += frames
        acc[1] += exposed
    for label, (frames, exposed) in by_label.items():
        print(f"all {label}: exposed {exposed} of {frames} face frames ({exposed / frames:.1%})")
    if gate:
        expected = load(FIXTURES / "expected.json")
        failed = False
        for label, (frames, exposed) in by_label.items():
            if label not in expected:
                continue
            limit = expected[label] + GATE * frames
            verdict = "ok" if exposed <= limit else "FAILED"
            failed |= exposed > limit
            print(f"gate {label}: {exposed} against {expected[label]} recorded, at most "
                  f"{limit:.0f}: {verdict}")
        if "legacy" in by_label and "tiles-off" in by_label:
            engine, legacy = by_label["tiles-off"][1], by_label["legacy"][1]
            verdict = "ok" if engine <= legacy else "FAILED"
            failed |= engine > legacy
            print(f"gate tiles-off against 1.12.1: {engine} against {legacy}: {verdict}")
        if failed:
            sys.exit(1)


if __name__ == "__main__":
    main()
