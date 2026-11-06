"""Write the corresponding source of the bundled FFmpeg build (plan 10.1): every `use = "source"`
archive of manifests/native.toml as downloaded, and the scripts that build them.

Usage: python -I tools/ffmpeg/source_bundle.py <out folder>

Writes <out folder>/CloakFrame-FFmpeg-<version>-<revision>-source.tar and prints its path. The
archives keep the names tools/fetch_native.py caches them under, so pointing
CLOAKFRAME_NATIVE_CACHE at the unpacked `sources` folder builds without a network. The tar holds
the same bytes on every run. CLOAKFRAME_NATIVE_CACHE names a folder to keep the downloads in.
"""

import io
import os
import sys
import tarfile
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.dont_write_bytecode = True
import fetch_native  # noqa: E402

SCRIPTS = [
    "tools/ffmpeg/build.sh",
    "tools/ffmpeg/check.sh",
    "tools/ffmpeg/components.sh",
    "tools/ffmpeg/revision",
    "tools/ffmpeg/source_bundle.py",
    "tools/fetch_native.py",
    "manifests/native.toml",
]

README = """\
The corresponding source of the ffmpeg and ffprobe programs in CloakFrame {version}-{revision}
packages: FFmpeg {version} with x264, x265 and, depending on the platform, NVIDIA's NVENC headers
and Intel's libvpl. FFmpeg built this way is licensed under the GNU General Public License,
version 2 or later.

sources/   the upstream archives, unchanged (their URLs and SHA-256 are in manifests/native.toml)
tools/     the scripts that build them; tools/ffmpeg/components.sh lists what is enabled

To build, unpack this file, then on the system tools/ffmpeg/build.sh names for the platform:

    CLOAKFRAME_NATIVE_CACHE=$PWD/sources tools/ffmpeg/build.sh <platform> <out>

where <platform> is macos-arm64, linux-x86_64 or windows-x64.
"""


def add(tar, name, data, mode=0o644):
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mode = mode
    info.mtime = 0
    info.uname = info.gname = ""
    tar.addfile(info, io.BytesIO(data))


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    out = Path(sys.argv[1]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temporary:
        write(out, Path(os.environ.get("CLOAKFRAME_NATIVE_CACHE", temporary)))


def write(out: Path, cache: Path) -> None:
    manifest = tomllib.loads((ROOT / "manifests" / "native.toml").read_text(encoding="utf-8"))
    sources = [a for a in manifest["artifact"] if a.get("use") == "source"]
    version = next(a["version"] for a in sources if a["id"] == "ffmpeg-source")
    revision = (ROOT / "tools" / "ffmpeg" / "revision").read_text(encoding="utf-8").strip()
    top = f"CloakFrame-FFmpeg-{version}-{revision}-source"

    archives = [
        fetch_native.fetch(a["id"], a["platform"].split(",")[0].strip(), cache, archive_only=True)
        for a in sources
    ]

    bundle = out / f"{top}.tar"
    with tarfile.open(bundle, "w", format=tarfile.PAX_FORMAT) as tar:
        add(tar, f"{top}/README.txt", README.format(version=version, revision=revision).encode())
        for path in sorted(archives, key=lambda p: p.name):
            add(tar, f"{top}/sources/{path.name}", path.read_bytes())
        for script in SCRIPTS:
            path = ROOT / script
            mode = 0o755 if path.suffix in (".sh", ".py") else 0o644
            add(tar, f"{top}/{script}", path.read_bytes(), mode)
    print(bundle)


if __name__ == "__main__":
    main()
