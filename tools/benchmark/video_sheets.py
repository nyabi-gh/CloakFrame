"""Crop and frame sheets for tagging the video benchmark's reference faces by eye (docs/M5_BENCHMARK.md).

python3 tools/benchmark/video_sheets.py boxes <ffmpeg> <ffprobe> <clip> <every> <out dir> RUN...
    Every box of the runs' face tracks on frames that are multiples of <every>, one per place
    (boxes overlapping by IoU 0.5 or more are one), as numbered crops in sheets of 48, and
    index.json.
python3 tools/benchmark/video_sheets.py audit <ffmpeg> <ffprobe> <clip> <every> <out dir> <index.json>
        [<top> <bottom>]
    Frames that are multiples of <every> with the numbered boxes and a 100 px grid, for faces no
    run found: the rows from <top> to <bottom> (where people are; all by default) in pieces 480 px
    wide, doubled.
python3 tools/benchmark/video_sheets.py reference <index.json> <tags.json> <clip name> <frames>
        <width> <height> <sample every> <audit every> <out>
    The reference: tags.json is {"face": [n], "unsure": [n], "added": [[frame, x, y, w, h, tag]]}.

Needs Pillow; run it in a virtual environment, not with -I.
"""

import gzip
import json
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw


def load(path):
    opener = gzip.open if str(path).endswith(".gz") else open
    with opener(path, "rt", encoding="utf-8") as f:
        return json.load(f)


def iou(a, b):
    iw = max(0.0, min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0]))
    ih = max(0.0, min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1]))
    inter = iw * ih
    union = a[2] * a[3] + b[2] * b[3] - inter
    return inter / union if union > 0 else 0.0


def size_of(ffprobe, clip):
    out = subprocess.check_output([ffprobe, "-v", "error", "-select_streams", "v:0",
                                   "-show_entries", "stream=width,height", "-of", "json", clip])
    s = json.loads(out)["streams"][0]
    return s["width"], s["height"]


def frames_of(ffmpeg, ffprobe, clip, wanted):
    """The wanted frame numbers as RGB images, decoded in one pass with every frame kept."""
    w, h = size_of(ffprobe, clip)
    proc = subprocess.Popen([ffmpeg, "-v", "error", "-i", clip, "-fps_mode", "passthrough",
                             "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], stdout=subprocess.PIPE)
    out, n, last = {}, 0, max(wanted)
    while n <= last:
        buf = proc.stdout.read(w * h * 3)
        if len(buf) < w * h * 3:
            break
        if n in wanted:
            out[n] = Image.frombytes("RGB", (w, h), buf)
        n += 1
    proc.kill()
    return out


def boxes(ffmpeg, ffprobe, clip, every, out_dir, runs):
    found = {}
    for path in runs:
        for track in load(path)["tracks"]:
            if track.get("kind", "face") != "face":
                continue
            for f, x, y, w, h, score, _ in track["boxes"]:
                if f % every == 0:
                    found.setdefault(f, []).append((score, [x, y, w, h]))
    entries = []
    for f in sorted(found):
        kept = []
        for _, b in sorted(found[f], key=lambda e: -e[0]):
            if all(iou(b, k) < 0.5 for k in kept):
                kept.append(b)
        entries += [(f, b) for b in kept]
    images = frames_of(ffmpeg, ffprobe, clip, {f for f, _ in entries})
    out_dir.mkdir(parents=True, exist_ok=True)
    tile, cols, rows = 110, 8, 6
    for s in range(0, len(entries), cols * rows):
        sheet = Image.new("RGB", (cols * tile, rows * (tile + 12)), "white")
        draw = ImageDraw.Draw(sheet)
        for k, (f, (x, y, w, h)) in enumerate(entries[s:s + cols * rows]):
            im = images[f]
            crop = im.crop((int(max(0, x - 0.4 * w)), int(max(0, y - 0.4 * h)),
                            int(min(im.width, x + 1.4 * w)), int(min(im.height, y + 1.4 * h))))
            scale = tile / max(crop.width, crop.height)
            crop = crop.resize((max(1, int(crop.width * scale)), max(1, int(crop.height * scale))))
            cx, cy = (k % cols) * tile, (k // cols) * (tile + 12)
            sheet.paste(crop, (cx, cy))
            draw.text((cx + 2, cy + tile), f"{s + k} {int(h)}px", fill="black")
        sheet.save(out_dir / f"boxes-{s // (cols * rows):03d}.png")
    index = [[n, f, [round(v, 1) for v in b]] for n, (f, b) in enumerate(entries)]
    (out_dir / "index.json").write_text(json.dumps(index))
    print(f"{len(entries)} boxes on {len(images)} frames")


def audit(ffmpeg, ffprobe, clip, every, out_dir, index_path, top=0, bottom=None):
    index = load(index_path)
    w, h = size_of(ffprobe, clip)
    count = int(subprocess.check_output([
        ffprobe, "-v", "error",
        "-count_packets", "-select_streams", "v:0", "-show_entries", "stream=nb_read_packets",
        "-of", "csv=p=0", clip]).decode().strip().rstrip(","))
    images = frames_of(ffmpeg, ffprobe, clip, set(range(0, count, every)))
    out_dir.mkdir(parents=True, exist_ok=True)
    for f, im in sorted(images.items()):
        draw = ImageDraw.Draw(im)
        for x in range(0, w, 100):
            draw.line([(x, 0), (x, h)], fill=(255, 255, 0), width=1)
            draw.text((x + 2, 2), str(x), fill=(255, 255, 0))
        for y in range(0, h, 100):
            draw.line([(0, y), (w, y)], fill=(255, 255, 0), width=1)
            draw.text((2, y + 2), str(y), fill=(255, 255, 0))
        for n, frame, (x, y, bw, bh) in index:
            if frame == f:
                draw.rectangle([x, y, x + bw, y + bh], outline=(0, 255, 0), width=1)
                draw.text((x, max(0, y - 11)), str(n), fill=(0, 255, 0))
        low = h if bottom is None else bottom
        for i, left in enumerate(range(0, w, 480)):
            piece = im.crop((left, top, min(w, left + 480), low))
            piece = piece.resize((piece.width * 2, piece.height * 2), Image.LANCZOS)
            piece.save(out_dir / f"frame-{f:05d}-{i}.png")
    print(f"{len(images)} frames")


def reference(index_path, tags_path, name, frames, width, height, sample, audit_every, out):
    index = {n: (f, b) for n, f, b in load(index_path)}
    tags = load(tags_path)
    faces = []
    for tag in ("face", "unsure"):
        for n in tags[tag]:
            f, b = index[n]
            faces.append([f, *b, tag])
    taken = {}
    for face in sorted(faces):
        same = taken.setdefault(face[0], [])
        if all(iou(face[1:5], o[1:5]) < 0.3 for o in same):
            same.append(face)
    doc = {"clip": name, "frames": int(frames), "width": int(width), "height": int(height),
           "sample_every": int(sample), "audit_every": int(audit_every),
           "faces": [f for fs in taken.values() for f in fs], "added": tags.get("added", [])}
    Path(out).write_text(json.dumps(doc, separators=(",", ":")) + "\n")
    print(f"{len(doc['faces'])} faces, {len(doc['added'])} added")


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "boxes":
        boxes(args[0], args[1], args[2], int(args[3]), Path(args[4]), args[5:])
    elif cmd == "audit":
        band = [int(v) for v in args[6:8]]
        audit(args[0], args[1], args[2], int(args[3]), Path(args[4]), args[5], *band)
    elif cmd == "reference":
        reference(*args)
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
