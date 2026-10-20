"""Contact sheets for tagging the benchmark by hand, and writing the tags (docs/M3_BENCHMARK.md).

Usage:
  python3 tools/benchmark/sheets.py [--manifest M] boxes <images dir> <out dir>
  python3 tools/benchmark/sheets.py [--manifest M] unmatched <images dir> <out dir> RESULT...
  python3 tools/benchmark/sheets.py tag <out dir> [--profile N..] [--oblique N..]
                                    [--unclear N..] [--real N..]

M is tests/fixtures/benchmark/manifest.json (Open Images' copies) or originals.json.

boxes: numbered crops of the counted faces at least MIN_FACE px tall and plates at least MIN_PLATE
px wide not tagged yet, for telling frontal from profile faces and plates seen head-on from oblique
ones; also finds the counted faces whose mean luma (BT.601 on the sRGB pixels) is below DARK.

unmatched: numbered crops of the detections of score REVIEW_SCORE or more in the results that
evaluate.py counts as false, one per place, for telling real faces and plates the labels lack
from false detections.

tag: records what the sheets in <out dir> showed in tests/fixtures/benchmark/tags.json. N.. are
cell numbers or ranges (3,5,10-12). Faces and plates not listed are frontal; --unclear marks crops
too dark, small or hidden to judge; --real adds a crop of an unmatched sheet as an unlabeled face
or plate.

Needs Pillow.
"""

import argparse
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageOps, ImageStat

import evaluate

ROOT = Path(__file__).resolve().parent.parent.parent
FIXTURES = ROOT / "tests" / "fixtures" / "benchmark"
MIN_FACE = 24
MIN_PLATE = 24
DARK = 60
REVIEW_SCORE = 0.3
COLUMNS, ROWS = 8, 6
TAGS_COMMENT = (
    "Tags made by hand from tools/benchmark/sheets.py, for manifest.json and originals.json: "
    "boxes.<index> of an image's boxes (pose, angle; dark is computed), added: real faces and "
    "plates the Open Images labels lack, as fractions of the width and height."
)


def load(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def crop(image: Image.Image, box, margin: float, cell) -> Image.Image:
    x, y, w, h = box
    mx, my = w * margin, h * margin
    region = image.crop((int(x - mx), int(y - my), int(x + w + mx) + 1, int(y + h + my) + 1))
    return ImageOps.pad(region, cell, color=(40, 40, 40))


def write_sheets(cells: list, cell, prefix: str, out: Path, first: int = 0) -> list:
    """Saves the crops of `cells` ((entry, picture) pairs) as sheets numbered from `first`; returns
    the entries with their numbers."""
    index = []
    per_sheet = COLUMNS * ROWS
    for start in range(0, len(cells), per_sheet):
        page = Image.new("RGB", (COLUMNS * cell[0], ROWS * (cell[1] + 14)), (255, 255, 255))
        draw = ImageDraw.Draw(page)
        for n, (entry, picture) in enumerate(cells[start : start + per_sheet]):
            number = first + len(index)
            col, row = n % COLUMNS, n // COLUMNS
            page.paste(picture, (col * cell[0], row * (cell[1] + 14) + 14))
            label = str(number)
            if "score" in entry:
                label += f" {entry['kind'][0].upper()} {entry['score']:.2f}"
            draw.text((col * cell[0] + 2, row * (cell[1] + 14)), label, fill=(0, 0, 0))
            index.append({"cell": number, **entry})
        page.save(out / f"{prefix}-{start // per_sheet:02}.png")
    return index


def opened(images: Path, name: str) -> Image.Image:
    with Image.open(images / name) as image:
        return image.convert("RGB")


def boxes(manifest_path: Path, images: Path, out: Path) -> None:
    manifest, tags = evaluate.load_set(manifest_path, FIXTURES / "tags.json")
    faces, plates, dark = [], [], {}
    for entry in manifest["images"]:
        tagged = tags.get(entry["name"], {}).get("boxes", {})
        counted = [
            (i, b)
            for i, b in enumerate(entry["boxes"])
            if not b["group"]
            and not b["depiction"]
            and not {"pose", "angle"} & tagged.get(str(i), {}).keys()
        ]
        if not counted:
            continue
        image = opened(images, entry["name"])
        for i, b in counted:
            x, y, w, h = b["box"]
            cell = {"image": entry["name"], "box": i}
            if b["kind"] == "face":
                region = image.crop((int(x), int(y), int(x + w) + 1, int(y + h) + 1)).convert("L")
                luma = ImageStat.Stat(region).mean[0]
                if luma < DARK:
                    dark.setdefault(entry["name"], []).append(i)
                if h >= MIN_FACE:
                    faces.append(({**cell, "kind": "face"}, crop(image, b["box"], 0.25, (112, 112))))
            elif w >= MIN_PLATE:
                plates.append(({**cell, "kind": "plate"}, crop(image, b["box"], 0.6, (160, 96))))
    cells = write_sheets(faces, (112, 112), "faces", out)
    cells += write_sheets(plates, (160, 96), "plates", out, first=len(cells))
    (out / "cells.json").write_text(
        json.dumps({"sheets": "boxes", "cells": cells, "dark": dark}, indent=1) + "\n", "utf-8"
    )
    print(f"{len(faces)} faces, {len(plates)} plates, {sum(map(len, dark.values()))} dark faces")


def unmatched(manifest_path: Path, images: Path, out: Path, results: list) -> None:
    manifest, tags = evaluate.load_set(manifest_path, FIXTURES / "tags.json")
    found = [{r["name"]: r for r in load(p)["images"]} for p in results]
    picked = []
    for entry in manifest["images"]:
        places = []
        for kind, verified in (("face", "faces"), ("plate", "plates")):
            if entry[verified] is None:
                continue
            for result in found:
                r = result.get(entry["name"])
                if r is None or r.get("failure"):
                    continue
                _, _, verdicts, _ = evaluate.classify(kind, entry, r["detections"], tags)
                for d, verdict in verdicts:
                    if verdict != "false" or d["score"] < REVIEW_SCORE:
                        continue
                    same = [p for p in places if p["kind"] == kind and evaluate.iou(p["box"], d["box"]) >= 0.5]
                    if not same:
                        places.append({"kind": kind, "box": d["box"], "score": d["score"]})
                    elif d["score"] > same[0]["score"]:
                        same[0].update(box=d["box"], score=d["score"])
        if places:
            image = opened(images, entry["name"])
            for p in places:
                picture = crop(image, p["box"], 0.5, (128, 128))
                picked.append(({"image": entry["name"], "size": entry["size"], **p}, picture))
    cells = write_sheets(picked, (128, 128), "unmatched", out)
    (out / "cells.json").write_text(
        json.dumps({"sheets": "unmatched", "cells": cells}, indent=1) + "\n", "utf-8"
    )
    print(f"{len(cells)} unmatched detections")


def numbers(text: str) -> set:
    out = set()
    for part in filter(None, (text or "").split(",")):
        low, _, high = part.partition("-")
        out.update(range(int(low), int(high or low) + 1))
    return out


def tag(out: Path, args) -> None:
    sheets = load(out / "cells.json")
    path = FIXTURES / "tags.json"
    tags = load(path) if path.exists() else {}
    tags["comment"] = TAGS_COMMENT
    unclear = numbers(args.unclear)
    if sheets["sheets"] == "boxes":
        for name, indices in sheets["dark"].items():
            for i in indices:
                tags.setdefault(name, {}).setdefault("boxes", {}).setdefault(str(i), {})["dark"] = True
        profile, oblique = numbers(args.profile), numbers(args.oblique)
        for c in sheets["cells"]:
            box = tags.setdefault(c["image"], {}).setdefault("boxes", {}).setdefault(str(c["box"]), {})
            key = "pose" if c["kind"] == "face" else "angle"
            if c["cell"] in unclear:
                box[key] = "unclear"
            elif c["kind"] == "face":
                box[key] = "profile" if c["cell"] in profile else "frontal"
            else:
                box[key] = "oblique" if c["cell"] in oblique else "frontal"
    else:
        for c in sheets["cells"]:
            if c["cell"] in numbers(args.real):
                added = tags.setdefault(c["image"], {}).setdefault("added", [])
                w, h = c["size"]
                x, y, bw, bh = c["box"]
                entry = {"kind": c["kind"], "box": [round(v, 6) for v in (x / w, y / h, bw / w, bh / h)]}
                if entry not in added:
                    added.append(entry)
    ordered = {"comment": tags.pop("comment")}
    ordered.update(sorted(tags.items()))
    path.write_text(json.dumps(ordered, indent=1) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--manifest", type=Path, default=FIXTURES / "manifest.json")
    sub = parser.add_subparsers(dest="command", required=True)
    b = sub.add_parser("boxes")
    b.add_argument("images", type=Path)
    b.add_argument("out", type=Path)
    u = sub.add_parser("unmatched")
    u.add_argument("images", type=Path)
    u.add_argument("out", type=Path)
    u.add_argument("results", nargs="+", type=Path)
    t = sub.add_parser("tag")
    t.add_argument("out", type=Path)
    for flag in ("--profile", "--oblique", "--unclear", "--real"):
        t.add_argument(flag)
    args = parser.parse_args()
    if args.command == "tag":
        tag(args.out, args)
        return
    args.out.mkdir(parents=True, exist_ok=True)
    if args.command == "boxes":
        boxes(args.manifest, args.images, args.out)
    else:
        unmatched(args.manifest, args.images, args.out, args.results)


if __name__ == "__main__":
    main()
