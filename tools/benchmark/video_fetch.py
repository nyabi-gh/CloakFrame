"""Download the video benchmark's sources, cut its clips and verify both by SHA-256.

Usage: python -I tools/benchmark/video_fetch.py <ffmpeg> <dest dir>

Each clip of tests/fixtures/benchmark/video/manifest.json comes from the first `bytes` of a
Wikimedia Commons file (a WebM's start decodes on its own), cut by stream copy or, for a source
1.12.1 and the engine refuse (HLG), encoded again with BT.709 tags. The cut needs the pinned FFmpeg
(9.0.2, tools/fetch_native.py) to give the recorded bytes. Sources and clips already in <dest dir>
with the right hash are kept. Prints the absolute path of <dest dir>.
"""

import hashlib
import json
import subprocess
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
MANIFEST = ROOT / "tests" / "fixtures" / "benchmark" / "video" / "manifest.json"
# Wikimedia asks clients to name themselves (https://meta.wikimedia.org/wiki/User-Agent_policy).
USER_AGENT = "cloakframe-benchmark (https://github.com/nyabi-gh/CloakFrame)"
# Same pixels with BT.709 tags, one encoder thread so the bytes do not depend on the machine.
RETAG = "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709"
ENCODE = ["-c:v", "libx264", "-preset", "medium", "-crf", "16", "-threads", "1", "-pix_fmt", "yuv420p",
          "-flags:v", "+bitexact"]


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def download(url: str, size: int, path: Path) -> None:
    """Saves the first `size` bytes of `url` at `path` through `path`.part."""
    partial = path.with_suffix(path.suffix + ".part")
    request = urllib.request.Request(
        url, headers={"User-Agent": USER_AGENT, "Range": f"bytes=0-{size - 1}"})
    with urllib.request.urlopen(request, timeout=120) as response, partial.open("wb") as out:
        while chunk := response.read(1 << 20):
            out.write(chunk)
    partial.replace(path)


def verified(path: Path, digest: str, what: str) -> None:
    actual = sha256(path)
    if actual != digest:
        path.unlink()
        raise SystemExit(f"SHA-256 mismatch for {what}: {actual}")


def cut(ffmpeg: str, source: Path, clip: dict, out: Path) -> None:
    c = clip["cut"]
    args = [ffmpeg, "-v", "error", "-y", "-ss", str(c["start"]), "-i", str(source),
            "-t", str(c["duration"]), "-map", "0:v:0", "-map_metadata", "-1", "-fflags", "+bitexact"]
    args += ["-vf", RETAG] + ENCODE if c.get("retag") else ["-c", "copy"]
    subprocess.run(args + [str(out)], check=True)


def fetch(ffmpeg: str, clip: dict, dest: Path) -> None:
    source = dest / "sources" / clip["source"]["name"]
    out = dest / clip["file"]
    if out.exists() and sha256(out) == clip["sha256"]:
        return
    if not (source.exists() and sha256(source) == clip["source"]["sha256"]):
        download(clip["source"]["url"], clip["source"]["bytes"], source)
        verified(source, clip["source"]["sha256"], clip["source"]["url"])
    cut(ffmpeg, source, clip, out)
    verified(out, clip["sha256"], f"the cut of {clip['name']} (is FFmpeg 9.0.2?)")


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    ffmpeg = sys.argv[1]
    dest = Path(sys.argv[2]).resolve()
    (dest / "sources").mkdir(parents=True, exist_ok=True)
    for clip in json.loads(MANIFEST.read_text(encoding="utf-8"))["clips"]:
        fetch(ffmpeg, clip, dest)
    print(dest)


if __name__ == "__main__":
    main()
