"""Choose the detection benchmark set (plan 9.2) from Open Images V7 and write its manifests.

Usage: python -I tools/benchmark/pick.py <work dir> <out dir> [--originals | --cached-originals]

Downloads the Open Images validation and test annotations into <work dir> and draws images for
each pool with a fixed seed. The set is Open Images' own copies of the photos (COPIES, long side
1,024 px), kept when they are baseline or progressive JPEGs with three or one components and
upright (no rotation, EXIF orientation 1 or none): <out dir>/manifest.json.

With --originals, the face pools' Flickr originals go in <out dir>/originals.json, for measuring
tile mode at the sizes people's photos have: each is kept when it is the file Open Images labeled
(its size and MD5), upright and at most MAX_BYTES and MAX_PIXELS. Flickr limits how many originals a client may
fetch (docs/M3_BENCHMARK.md), so these take a while; fetch.py waits the limit out.
--cached-originals uses only the originals already in <work dir>/originals, for when the limit
lasts longer than is worth waiting.

Boxes are fractions of the image's width and height. Images are CC BY 2.0 as Open Images lists
them; the boxes are Google's, CC BY 4.0.
"""

import base64
import csv
import hashlib
import importlib.util
import json
import random
import struct
import sys
import threading
import urllib.error
from collections import deque
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

OPEN_IMAGES = "https://storage.googleapis.com/openimages"
COPIES = "https://open-images-dataset.s3.amazonaws.com"
SPLITS = {
    "validation": {
        "boxes": f"{OPEN_IMAGES}/v5/validation-annotations-bbox.csv",
        "labels": f"{OPEN_IMAGES}/v5/validation-annotations-human-imagelabels-boxable.csv",
        "images": f"{OPEN_IMAGES}/2018_04/validation/validation-images-with-rotation.csv",
    },
    "test": {
        "boxes": f"{OPEN_IMAGES}/v5/test-annotations-bbox.csv",
        "labels": f"{OPEN_IMAGES}/v5/test-annotations-human-imagelabels-boxable.csv",
        "images": f"{OPEN_IMAGES}/2018_04/test/test-images-with-rotation.csv",
    },
}
FACE = "/m/0dzct"
PLATE = "/m/01jfm_"
PERSON = "/m/01g317"
CAR = "/m/0k4j"
CC_BY_2 = "https://creativecommons.org/licenses/by/2.0/"
MAX_PIXELS = 24_000_000
MAX_BYTES = 12 << 20
CROWD = 8
SEED = 20261020
# Pools in the order they are filled; an image is in the first pool it qualifies for.
TARGETS = {"crowd": 60, "faces": 240, "plates": 150, "no-faces": 100, "no-plates": 60}
FACE_POOLS = ("crowd", "faces")
COMMENT = (
    "Detection benchmark (docs/M3_BENCHMARK.md), made by tools/benchmark/pick.py. Images: {what}, "
    "CC BY 2.0 by their authors, fetched by hash and not stored here. Boxes: Open Images V7 by "
    "Google LLC, CC BY 4.0, as fractions of the width and height. faces/plates: the "
    "human-verified image label (true, false or null)."
)


def load_fetch():
    """tools/benchmark/fetch.py, which downloads the images with Flickr's rate limit kept."""
    spec = importlib.util.spec_from_file_location("fetch", Path(__file__).with_name("fetch.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read_csv(path: Path):
    with path.open(encoding="utf-8", newline="") as f:
        yield from csv.DictReader(f)


def jpeg_facts(data: bytes):
    """(width, height, components, EXIF orientation) of a JPEG, or None when it is not one."""
    if not data.startswith(b"\xff\xd8"):
        return None
    pos, orientation, frame = 2, None, None
    while pos + 4 <= len(data):
        if data[pos] != 0xFF:
            return None
        marker = data[pos + 1]
        if marker == 0xFF:
            pos += 1
            continue
        if marker in (0xD8, 0x01) or 0xD0 <= marker <= 0xD7:
            pos += 2
            continue
        length = struct.unpack(">H", data[pos + 2 : pos + 4])[0]
        segment = data[pos + 4 : pos + 2 + length]
        if marker == 0xE1 and segment.startswith(b"Exif\0\0"):
            orientation = exif_orientation(segment[6:])
        elif marker in (0xC0, 0xC1, 0xC2):
            height, width = struct.unpack(">HH", segment[1:5])
            frame = (width, height, segment[5])
        elif marker == 0xDA:
            break
        pos += 2 + length
    if frame is None:
        return None
    return (*frame, orientation)


def exif_orientation(tiff: bytes):
    if len(tiff) < 8 or tiff[:2] not in (b"II", b"MM"):
        return None
    order = "<" if tiff[:2] == b"II" else ">"
    (ifd,) = struct.unpack(order + "I", tiff[4:8])
    if ifd + 2 > len(tiff):
        return None
    (count,) = struct.unpack(order + "H", tiff[ifd : ifd + 2])
    for i in range(count):
        entry = tiff[ifd + 2 + 12 * i : ifd + 14 + 12 * i]
        if len(entry) < 12:
            return None
        tag, kind = struct.unpack(order + "HH", entry[:4])
        if tag == 0x0112 and kind == 3:
            return struct.unpack(order + "H", entry[8:10])[0]
    return None


def upright(data: bytes):
    """(width, height) of an upright one- or three-component JPEG, else None."""
    facts = jpeg_facts(data)
    if facts is None:
        return None
    width, height, components, orientation = facts
    if components not in (1, 3) or orientation not in (None, 1):
        return None
    return width, height


def main() -> None:
    flags = {"--originals", "--cached-originals"}
    args = [a for a in sys.argv[1:] if a not in flags]
    cached_only = "--cached-originals" in sys.argv
    if len(args) != 2:
        raise SystemExit(__doc__)
    work = Path(args[0]).resolve()
    out = Path(args[1]).resolve()
    for folder in ("copies", "originals"):
        (work / folder).mkdir(parents=True, exist_ok=True)
    fetch = load_fetch()
    throttle = fetch.Throttle()

    def get(url: str, path: Path) -> None:
        if not path.exists():
            fetch.download(url, path, throttle)

    meta, boxes, verified = {}, {}, {}
    for split, urls in SPLITS.items():
        for key, url in urls.items():
            get(url, work / f"{split}-{key}.csv")
        for r in read_csv(work / f"{split}-images.csv"):
            meta[r["ImageID"]] = {**r, "Subset": split}
        for r in read_csv(work / f"{split}-labels.csv"):
            if r["LabelName"] in (FACE, PLATE, PERSON, CAR):
                verified.setdefault(r["ImageID"], {})[r["LabelName"]] = r["Confidence"] == "1"
        for r in read_csv(work / f"{split}-boxes.csv"):
            if r["LabelName"] in (FACE, PLATE):
                boxes.setdefault(r["ImageID"], []).append(r)

    # Candidates have an original that could join originals.json, so both sets come from one draw.
    def usable(image_id: str) -> bool:
        m = meta.get(image_id)
        return (
            m is not None
            and m["License"] == CC_BY_2
            and m["Rotation"] in ("", "0", "0.0")
            and m["OriginalURL"].startswith("https://")
            and 0 < int(m["OriginalSize"] or 0) <= MAX_BYTES
        )

    def label(image_id: str, name: str):
        return verified.get(image_id, {}).get(name)

    def pool_of(image_id: str):
        faces = [b for b in boxes.get(image_id, []) if b["LabelName"] == FACE]
        if label(image_id, FACE):
            return "crowd" if len(faces) >= CROWD else "faces"
        if label(image_id, PLATE):
            return "plates"
        if label(image_id, FACE) is False:
            return "no-faces"
        if label(image_id, PLATE) is False:
            return "no-plates"
        return None

    pools = {name: [] for name in TARGETS}
    for image_id in sorted(verified):
        name = pool_of(image_id)
        if name and usable(image_id):
            pools[name].append(image_id)
    rng = random.Random(SEED)
    for name, ids in pools.items():
        rng.shuffle(ids)
        # No-face and no-plate images with people and cars test false detections where they
        # are likely.
        busy = PERSON if name == "no-faces" else CAR if name == "no-plates" else None
        if busy:
            ids.sort(key=lambda i: label(i, busy) is not True)

    def take_copy(image_id: str):
        m = meta[image_id]
        path = work / "copies" / f"{image_id}.jpg"
        get(f"{COPIES}/{m['Subset']}/{image_id}.jpg", path)
        data = path.read_bytes()
        size = upright(data)
        return (image_id, data, size) if size else None

    # The images kept are the first that qualify in the shuffled order.
    chosen = []
    with ThreadPoolExecutor(fetch.CONNECTIONS) as pool:
        for name, target in TARGETS.items():
            kept = []
            ahead = deque()
            ids = iter(pools[name])
            while len(kept) < target:
                while len(ahead) < 16 and (image_id := next(ids, None)) is not None:
                    ahead.append(pool.submit(take_copy, image_id))
                if not ahead:
                    break
                if result := ahead.popleft().result():
                    kept.append(result)
            for future in ahead:
                future.cancel()
            print(f"{name}: {len(kept)} of {target}", file=sys.stderr, flush=True)
            chosen += [(name, *k) for k in kept]

    def entry(name: str, image_id: str, url: str, data: bytes, size) -> dict:
        m = meta[image_id]
        e = {
            "name": f"{image_id}.jpg",
            "split": m["Subset"],
            "pool": name,
            "url": url,
            "sha256": hashlib.sha256(data).hexdigest(),
            "size": list(size),
            "license": m["License"],
            "author": m["Author"],
            "author_url": m["AuthorProfileURL"],
            "title": m["Title"],
            "page": m["OriginalLandingURL"],
            "faces": label(image_id, FACE),
            "plates": label(image_id, PLATE),
            "boxes": [],
        }
        for b in boxes.get(image_id, []):
            x0, x1, y0, y1 = (float(b[k]) for k in ("XMin", "XMax", "YMin", "YMax"))
            e["boxes"].append(
                {
                    "kind": "face" if b["LabelName"] == FACE else "plate",
                    "box": [round(v, 6) for v in (x0, y0, x1 - x0, y1 - y0)],
                    "occluded": b["IsOccluded"] == "1",
                    "truncated": b["IsTruncated"] == "1",
                    "group": b["IsGroupOf"] == "1",
                    "depiction": b["IsDepiction"] == "1",
                }
            )
        return e

    def write(path: Path, what: str, images: list) -> None:
        images.sort(key=lambda e: (list(TARGETS).index(e["pool"]), e["name"]))
        lines = ",\n".join(json.dumps(e, ensure_ascii=False) for e in images)
        comment = json.dumps(COMMENT.format(what=what))
        path.write_text(f'{{"comment": {comment},\n"images": [\n{lines}\n]}}\n', "utf-8")

    write(
        out / "manifest.json",
        "Open Images V7's copies of the Flickr photos",
        [
            entry(name, i, f"{COPIES}/{meta[i]['Subset']}/{i}.jpg", data, size)
            for name, i, data, size in chosen
        ],
    )
    if not flags & set(sys.argv):
        return

    # Originals Flickr no longer has are remembered so a rerun does not ask again. Any other
    # failure stops the run: skipping the image would make the set depend on the day.
    gone_file = work / "gone.txt"
    gone = set(gone_file.read_text().split()) if gone_file.exists() else set()
    gone_lock = threading.Lock()

    def take_original(image_id: str):
        m = meta[image_id]
        path = work / "originals" / f"{image_id}.jpg"
        if image_id in gone or (cached_only and not path.exists()):
            return None
        try:
            get(m["OriginalURL"], path)
        except urllib.error.HTTPError as e:
            if e.code not in (404, 410):
                raise
            with gone_lock, gone_file.open("a") as f:
                f.write(image_id + "\n")
            return None
        data = path.read_bytes()
        md5 = base64.b64encode(hashlib.md5(data).digest()).decode()
        if len(data) != int(m["OriginalSize"] or 0) or md5 != m["OriginalMD5"]:
            return None
        size = upright(data)
        if size is None or size[0] * size[1] > MAX_PIXELS:
            return None
        return image_id, data, size

    faces = [(name, i) for name, i, _, _ in chosen if name in FACE_POOLS]
    with ThreadPoolExecutor(fetch.CONNECTIONS) as pool:
        found = pool.map(take_original, [i for _, i in faces])
        originals = [
            entry(name, i, meta[i]["OriginalURL"], r[1], r[2])
            for (name, i), r in zip(faces, found)
            if r
        ]
    print(f"originals: {len(originals)} of {len(faces)}", file=sys.stderr)
    write(out / "originals.json", "the Flickr originals Open Images V7 lists", originals)


if __name__ == "__main__":
    main()
