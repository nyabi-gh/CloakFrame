"""Download the detection benchmark's images and verify their SHA-256.

Usage: python -I tools/benchmark/fetch.py <dest dir> [manifest]

The manifest is tests/fixtures/benchmark/manifest.json (Open Images' copies) unless given, for
example originals.json (Flickr originals, rate limited). Images already in <dest dir> with the
right hash are not fetched again. Prints the absolute path of <dest dir>.
"""

import hashlib
import json
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
# Flickr answers 429 once a client has asked for a hundred or more originals and keeps doing so for
# an hour or more (docs/M3_BENCHMARK.md). Every download then waits, so the limit is kept, not worked
# around.
RATE_LIMIT_WAIT = 300
MAX_WAIT = 4 * 3600
CONNECTIONS = 4


class Throttle:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._until = 0.0

    def wait(self) -> None:
        while (delay := self._until - time.monotonic()) > 0:
            time.sleep(delay)

    def back_off(self, seconds: float) -> None:
        with self._lock:
            self._until = max(self._until, time.monotonic() + seconds)


def download(url: str, path: Path, throttle: Throttle) -> None:
    """Saves `url` at `path` through `path`.part, waiting out rate limits."""
    partial = path.with_suffix(path.suffix + ".part")
    request = urllib.request.Request(url, headers={"User-Agent": "cloakframe-benchmark"})
    waited = 0
    while True:
        throttle.wait()
        try:
            with urllib.request.urlopen(request, timeout=120) as response, partial.open("wb") as out:
                while chunk := response.read(1 << 20):
                    out.write(chunk)
            partial.replace(path)
            return
        except urllib.error.HTTPError as e:
            if e.code != 429 or waited >= MAX_WAIT:
                raise
            after = e.headers.get("Retry-After", "")
            delay = int(after) if after.isdigit() else RATE_LIMIT_WAIT
            print(f"rate limited; waiting {delay} s", file=sys.stderr, flush=True)
            throttle.back_off(delay)
            waited += delay


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fetch(image: dict, dest: Path, throttle: Throttle) -> None:
    path = dest / image["name"]
    if path.exists() and sha256(path) == image["sha256"]:
        return
    download(image["url"], path, throttle)
    digest = sha256(path)
    if digest != image["sha256"]:
        path.unlink()
        raise SystemExit(f"SHA-256 mismatch for {image['url']}: {digest}")


def main() -> None:
    if len(sys.argv) not in (2, 3):
        raise SystemExit(__doc__)
    dest = Path(sys.argv[1]).resolve()
    dest.mkdir(parents=True, exist_ok=True)
    default = ROOT / "tests" / "fixtures" / "benchmark" / "manifest.json"
    manifest = Path(sys.argv[2]) if len(sys.argv) == 3 else default
    images = json.loads(manifest.read_text(encoding="utf-8"))["images"]
    throttle = Throttle()
    with ThreadPoolExecutor(CONNECTIONS) as pool:
        list(pool.map(lambda image: fetch(image, dest, throttle), images))
    print(dest)


if __name__ == "__main__":
    main()
