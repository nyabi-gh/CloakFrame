"""Download one artifact from manifests/native.toml, verify its SHA-256 and unpack it.

Usage: python -I tools/fetch_native.py <id> <platform> <dest dir>

Prints the absolute path of the artifact's `library` entry (or of the unpacked folder when it has
none). Downloads are cached in <dest dir> by hash, so a second call does not fetch again.
"""

import hashlib
import sys
import tarfile
import tomllib
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    wanted, platform, dest = sys.argv[1], sys.argv[2], Path(sys.argv[3]).resolve()
    manifest = tomllib.loads((ROOT / "manifests" / "native.toml").read_text(encoding="utf-8"))
    matches = [
        a for a in manifest["artifact"]
        if a["id"] == wanted and platform in [p.strip() for p in a["platform"].split(",")]
    ]
    if len(matches) != 1:
        raise SystemExit(f"{len(matches)} artifacts match {wanted} on {platform}")
    artifact = matches[0]

    dest.mkdir(parents=True, exist_ok=True)
    archive = dest / f"{artifact['sha256']}-{artifact['url'].rsplit('/', 1)[-1]}"
    if not archive.exists():
        partial = archive.with_suffix(archive.suffix + ".part")
        request = urllib.request.Request(artifact["url"], headers={"User-Agent": "cloakframe-ci"})
        with urllib.request.urlopen(request, timeout=300) as response, partial.open("wb") as out:
            while chunk := response.read(1 << 20):
                out.write(chunk)
        partial.replace(archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != artifact["sha256"]:
        archive.unlink()
        raise SystemExit(f"SHA-256 mismatch for {artifact['url']}: {digest}")

    unpacked = dest / f"{wanted}-{platform}-{artifact['version']}"
    if not unpacked.exists():
        staging = unpacked.with_name(unpacked.name + ".part")
        staging.mkdir()
        name = archive.name
        if name.endswith((".tgz", ".tar.gz", ".tar.xz")):
            with tarfile.open(archive) as tar:
                tar.extractall(staging, filter="data")
        elif name.endswith((".zip", ".nupkg")):
            with zipfile.ZipFile(archive) as z:
                z.extractall(staging)
                # extractall drops Unix modes, which leaves the macOS FFmpeg zips' tools
                # unexecutable.
                for info in z.infolist():
                    mode = info.external_attr >> 16
                    if info.create_system == 3 and mode & 0o111 and not info.is_dir():
                        (staging / info.filename).chmod(mode & 0o777)
        else:
            (staging / name.split("-", 1)[1]).write_bytes(archive.read_bytes())
        staging.replace(unpacked)

    library = unpacked / artifact["library"] if "library" in artifact else unpacked
    if not library.exists():
        raise SystemExit(f"{library} is not in the archive")
    print(library)


if __name__ == "__main__":
    main()
