"""Score benchmark results (docs/M3_BENCHMARK.md) against the Open Images boxes.

Usage: python -I tools/benchmark/evaluate.py [--json OUT] [--gate BEFORE NEW]... [RESULT...]

RESULT is the JSON the engine's benchmark example or tools/benchmark/legacy writes. Each result is
scored for the kinds it detects, on the images whose label for that kind was verified:

- A box counts unless it is a group-of box or a depiction. A detection matches the unmatched
  counted box of its kind with the highest IoU of at least MATCH_IOU, highest score first.
- One that matches nothing is not a false detection when it lies on another box of its kind (a
  second box on one face, a depiction, inside a group-of box) or on a real face the labels lack,
  which tests/fixtures/benchmark/tags.json adds after a look at the crops (tools/benchmark/
  sheets.py); those are counted as unlabeled faces found.
- Recall is matched counted boxes over counted boxes; precision is matches over matches and false
  detections; the false detections of images verified to have none of the kind are also given per
  image.

Strata come from the boxes (size, occlusion, crowd) and from tags.json (pose, plate angle, dark).
--gate fails when NEW's recall of a kind is more than GATE_DROP below BEFORE's: 1.12.1's result
at the same settings (plan 9.2) or the engine's recorded one.
"""

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
FIXTURES = ROOT / "tests" / "fixtures" / "benchmark"
MATCH_IOU = 0.5
ON_BOX = 0.5
GATE_DROP = 0.01


def iou(a, b) -> float:
    w = min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0])
    h = min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1])
    if w <= 0 or h <= 0:
        return 0.0
    inter = w * h
    return inter / (a[2] * a[3] + b[2] * b[3] - inter)


def share_inside(a, b) -> float:
    w = min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0])
    h = min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1])
    if w <= 0 or h <= 0 or a[2] * a[3] <= 0:
        return 0.0
    return w * h / (a[2] * a[3])


def load_set(manifest_path: Path, tags_path: Path):
    """The manifest and its tags with every box in pixels of the manifest's images; both files
    keep boxes as fractions of the width and height, so the copies and the originals share them."""
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    tags = json.loads(tags_path.read_text(encoding="utf-8")) if tags_path.exists() else {}
    images, image_tags = [], {}
    for image in manifest["images"]:
        w, h = image["size"]

        def pixels(b, w=w, h=h):
            return [b[0] * w, b[1] * h, b[2] * w, b[3] * h]

        images.append({**image, "boxes": [{**b, "box": pixels(b["box"])} for b in image["boxes"]]})
        if t := tags.get(image["name"]):
            added = [{**a, "box": pixels(a["box"])} for a in t.get("added", [])]
            image_tags[image["name"]] = {**t, "added": added}
    return {**manifest, "images": images}, image_tags


def size_band(pixels: float, edges) -> str:
    for low, high in zip(edges, edges[1:]):
        if pixels < high:
            return f"{low}-{high}"
    return f"{edges[-1]}+"


FACE_SOURCE = (0, 16, 32, 64, 128, 256)
FACE_WHOLE_VIEW = (0, 8, 16, 32, 64)
PLATE_SOURCE = (0, 32, 64, 128)


def strata(kind: str, box: dict, image: dict, tags: dict) -> list:
    width, height = image["size"]
    x, y, w, h = box["box"]
    out = []
    if kind == "face":
        out.append(f"size {size_band(h, FACE_SOURCE)} px")
        whole = h * 640 / max(width, height)
        out.append(f"whole view {size_band(whole, FACE_WHOLE_VIEW)} px")
        if image["pool"] == "crowd":
            out.append("crowd")
        if tags.get("pose") in ("profile", "frontal"):
            out.append(tags["pose"])
    else:
        out.append(f"width {size_band(w, PLATE_SOURCE)} px")
        if tags.get("angle") in ("oblique", "frontal"):
            out.append(tags["angle"])
    if box["occluded"]:
        out.append("occluded")
    if box["truncated"]:
        out.append("truncated")
    if tags.get("dark"):
        out.append("dark")
    return out


def on_box(d, b) -> bool:
    """Whether detection box `d` lies on box `b`: matches it loosely or mostly overlaps it."""
    return (
        iou(d, b) >= MATCH_IOU
        or share_inside(d, b) >= ON_BOX
        or (share_inside(b, d) >= ON_BOX and d[2] * d[3] <= 4 * b[2] * b[3])
    )


def classify(kind: str, image: dict, detections: list, tags: dict):
    """The counted boxes the detections match, and each detection's verdict: "match", "labeled"
    (on a box of its kind that it does not match), "added" (on a face tags.json adds) or "false"."""
    boxes = [(i, b) for i, b in enumerate(image["boxes"]) if b["kind"] == kind]
    main = [(i, b) for i, b in boxes if not b["group"] and not b["depiction"]]
    groups = [b["box"] for _, b in boxes if b["group"]]
    others = [b["box"] for _, b in boxes if not b["group"]]
    added = [a["box"] for a in tags.get(image["name"], {}).get("added", []) if a["kind"] == kind]
    hit = set()
    verdicts = []
    for d in sorted((d for d in detections if d["kind"] == kind), key=lambda d: -d["score"]):
        best, best_iou = None, MATCH_IOU
        for i, b in main:
            if i in hit:
                continue
            v = iou(d["box"], b["box"])
            if v >= best_iou:
                best, best_iou = i, v
        if best is not None:
            hit.add(best)
            verdicts.append((d, "match"))
        elif any(on_box(d["box"], b) for b in others) or any(
            share_inside(d["box"], g) >= ON_BOX for g in groups
        ):
            verdicts.append((d, "labeled"))
        elif any(on_box(d["box"], a) for a in added):
            verdicts.append((d, "added"))
        else:
            verdicts.append((d, "false"))
    return main, hit, verdicts, added


def score_kind(kind: str, result: dict, manifest: dict, tags: dict) -> dict:
    found = {r["name"]: r for r in result["images"]}
    verified = "faces" if kind == "face" else "plates"
    counted = matched = false = extra = added_total = 0
    negative_images = negative_false = 0
    bands: dict = {}
    failures = []
    for image in manifest["images"]:
        if image[verified] is None:
            continue
        name = image["name"]
        r = found.get(name)
        if r is None or r.get("failure"):
            failures.append(name)
            continue
        main, hit, verdicts, added = classify(kind, image, r["detections"], tags)
        image_false = sum(v == "false" for _, v in verdicts)
        false += image_false
        added_total += len(added)
        extra += sum(any(on_box(d["box"], a) for d, v in verdicts if v == "added") for a in added)
        if not image[verified]:
            negative_images += 1
            negative_false += image_false
        box_tags = tags.get(name, {}).get("boxes", {})
        for i, b in main:
            counted += 1
            matched += i in hit
            for s in strata(kind, b, image, box_tags.get(str(i), {})):
                n, m = bands.get(s, (0, 0))
                bands[s] = (n + 1, m + (i in hit))
    return {
        "boxes": counted,
        "recall": matched / counted if counted else None,
        "precision": matched / (matched + false) if matched + false else None,
        "false": false,
        "false_per_negative_image": negative_false / negative_images if negative_images else None,
        "negative_images": negative_images,
        "unlabeled_found": extra,
        "unlabeled": added_total,
        "strata": {k: {"boxes": n, "recall": m / n} for k, (n, m) in sorted(bands.items())},
        "failures": failures,
    }


def evaluate(path: Path, manifest: dict, tags: dict) -> dict:
    result = json.loads(path.read_text(encoding="utf-8"))
    kinds = []
    if result["faces"]:
        kinds.append("face")
    if result["plates"]:
        kinds.append("plate")
    views = [r["views"] for r in result["images"]]
    return {
        "result": path.name,
        "detector": result["detector"],
        "faces": result["faces"],
        "plates": result["plates"],
        "thresholds": result["thresholds"],
        "tiles": result["tiles"],
        "seconds": result["seconds"],
        "views": sum(views),
        "images": len(views),
        "kinds": {k: score_kind(k, result, manifest, tags) for k in kinds},
    }


def percent(v) -> str:
    return "-" if v is None else f"{100 * v:.1f}"


def report(scores: list) -> None:
    for s in scores:
        t = s["thresholds"]
        print(
            f"\n## {s['result']}: {s['detector']}, tiles {'on' if s['tiles'] else 'off'}, "
            f"{s['seconds']:.1f} s, {s['views']} views for {s['images']} images"
        )
        for kind, k in s["kinds"].items():
            print(
                f"{kind} (score {t[kind]['score']:.2f}): recall {percent(k['recall'])}% of "
                f"{k['boxes']}, precision {percent(k['precision'])}%, {k['false']} false, "
                f"{k['false_per_negative_image'] or 0:.3f} per image without one, "
                f"{k['unlabeled_found']} of {k['unlabeled']} unlabeled found"
                + (f", failed {k['failures']}" if k["failures"] else "")
            )
            for name, band in k["strata"].items():
                print(f"  {name:24} {percent(band['recall']):>6}% of {band['boxes']}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("results", nargs="*", type=Path)
    parser.add_argument("--manifest", type=Path, default=FIXTURES / "manifest.json")
    parser.add_argument("--tags", type=Path, default=FIXTURES / "tags.json")
    parser.add_argument("--json", type=Path)
    parser.add_argument("--gate", nargs=2, type=Path, action="append", default=[])
    args = parser.parse_args()
    manifest, tags = load_set(args.manifest, args.tags)
    scores = [evaluate(p, manifest, tags) for p in args.results]
    report(scores)
    if args.json:
        args.json.write_text(json.dumps(scores, indent=1) + "\n", encoding="utf-8")
    failed = False
    for before_path, new in args.gate:
        a, b = evaluate(before_path, manifest, tags), evaluate(new, manifest, tags)
        for kind in a["kinds"]:
            before, after = a["kinds"][kind], b["kinds"].get(kind)
            if after is None or after["failures"]:
                print(f"gate: {new.name} has no complete {kind} result", file=sys.stderr)
                failed = True
            elif after["recall"] < before["recall"] - GATE_DROP:
                print(
                    f"gate: {kind} recall {percent(after['recall'])}% in {new.name} is more than "
                    f"{100 * GATE_DROP:.0f} points below {percent(before['recall'])}% in "
                    f"{before_path.name}",
                    file=sys.stderr,
                )
                failed = True
            else:
                print(
                    f"gate: {kind} recall {percent(after['recall'])}% in {new.name}, "
                    f"{percent(before['recall'])}% in {before_path.name}"
                )
    if failed:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
