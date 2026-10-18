"""Download the models and sample images of tests/fixtures/assets.json and verify their SHA-256.

Usage: python -I tools/fetch_test_assets.py <dest dir>

Files already in <dest dir> with the right hash are not fetched again. Prints the absolute path of
<dest dir>, which the engine's detector tests read from CLOAKFRAME_TEST_ASSETS.
"""

import hashlib
import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    dest = Path(sys.argv[1]).resolve()
    dest.mkdir(parents=True, exist_ok=True)
    spec = json.loads((ROOT / "tests" / "fixtures" / "assets.json").read_text(encoding="utf-8"))
    for asset in spec["models"] + spec["images"]:
        path = dest / asset["name"]
        if path.exists() and sha256(path) == asset["sha256"]:
            continue
        partial = path.with_suffix(path.suffix + ".part")
        request = urllib.request.Request(asset["url"], headers={"User-Agent": "cloakframe-ci"})
        with urllib.request.urlopen(request, timeout=300) as response, partial.open("wb") as out:
            while chunk := response.read(1 << 20):
                out.write(chunk)
        digest = sha256(partial)
        if digest != asset["sha256"]:
            partial.unlink()
            raise SystemExit(f"SHA-256 mismatch for {asset['url']}: {digest}")
        partial.replace(path)
    print(dest)


if __name__ == "__main__":
    main()
