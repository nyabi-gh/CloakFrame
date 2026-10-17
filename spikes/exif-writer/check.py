"""Write the allowlist EXIF into every container and check the files with independent readers.

Usage: python3 -I check.py <repo root> <work dir>

Readers: exiftool (values and -validate), Exiv2, Pillow, macOS ImageIO, libwebp
(webpinfo), libtiff (tiffinfo) and ImageMagick. Expected values come from
tests/fixtures/metadata/expected.json.
"""

import json
import subprocess
import sys
from pathlib import Path


def run(cmd, check=False):
    return subprocess.run(cmd, capture_output=True, text=True, check=check)


def exiftool_tags(path):
    tags = json.loads(run(["exiftool", "-j", "-G1", "-a", "-n", "-s", str(path)], check=True).stdout)[0]
    return tags


def validate(path):
    out = run(["exiftool", "-validate", "-warning", "-error", "-a", "-G1", "-s", str(path)]).stdout
    return [line.strip() for line in out.splitlines() if "Warning" in line or "Error" in line or "Validate" in line]


def exiv2_keys(path):
    out = run(["exiv2", "-q", "-pa", str(path)]).stdout
    return sorted({line.split()[0] for line in out.splitlines() if line.strip()})


def pillow(path):
    code = (
        "import sys, json\n"
        "from PIL import Image\n"
        "im = Image.open(sys.argv[1]); im.load()\n"
        "e = im.getexif()\n"
        "ifd = e.get_ifd(0x8769); gps = e.get_ifd(0x8825)\n"
        "print(json.dumps({'decoded': list(im.size), 'ifd0': len(e), 'exif': len(ifd), 'gps': len(gps),"
        " 'orientation': e.get(0x0112), 'make': e.get(0x010F)}))\n"
    )
    result = run([sys.executable, "-I", "-c", code, str(path)])
    return json.loads(result.stdout) if result.returncode == 0 else {"error": result.stderr.strip().splitlines()[-1:]}


IMAGEIO = None


def imageio(path):
    result = run([str(IMAGEIO), str(path)])
    return json.loads(result.stdout) if result.returncode == 0 else {"error": result.stderr.strip()}


def container(path):
    ext = path.suffix.lower()
    if ext == ".webp":
        result = run(["webpinfo", str(path)])
        lines = [line.strip() for line in result.stdout.splitlines() if line.strip().startswith(("Chunk", "Features present", "Errors detected", "No error"))]
        return {"webpinfo_exit": result.returncode, "summary": lines}
    if ext == ".tif":
        result = run(["tiffinfo", str(path)])
        return {"tiffinfo_exit": result.returncode, "stderr": result.stderr.strip().splitlines()[:3]}
    return {}


def magick(path):
    result = run(["magick", "identify", "-format", "%m %wx%h %[EXIF:Make] %[EXIF:Orientation]", str(path)])
    return {"exit": result.returncode, "out": result.stdout.strip(), "err": result.stderr.strip()[:200]}


def compare(tags, case):
    mismatched = {}
    for key, want in case["expect_exiftool_tags"].items():
        got = tags.get(key)
        if got != want and not (isinstance(want, float) and isinstance(got, (int, float)) and abs(got - want) < 1e-6):
            mismatched[key] = {"want": want, "got": got}
    present = [key for key in case.get("expect_absent", []) if key in tags]
    return mismatched, present


def main():
    repo = Path(sys.argv[1])
    work = Path(sys.argv[2])
    out = work / "out"
    out.mkdir(parents=True, exist_ok=True)
    meta = repo / "tests" / "fixtures" / "metadata"
    global IMAGEIO
    IMAGEIO = work / "imageio_read"
    run(["swiftc", "-O", "-o", str(IMAGEIO), str(Path(__file__).with_name("imageio_read.swift"))], check=True)
    expected = json.loads((meta / "expected.json").read_text())

    base = work / "base.png"
    run(["magick", str(meta / "rich-exif.jpg"), "-auto-orient", str(base)], check=True)
    rgba = work / "rgba.png"
    run(["magick", str(base), "-alpha", "set", "-channel", "A", "-evaluate", "set", "60%", "+channel", str(rgba)], check=True)
    extras = {
        "cwebp-lossy.webp": ["cwebp", "-quiet", "-q", "90", str(base), "-o"],
        "cwebp-lossless-alpha.webp": ["cwebp", "-quiet", "-lossless", str(rgba), "-o"],
        "cwebp-lossy-alpha.webp": ["cwebp", "-quiet", "-q", "90", str(rgba), "-o"],
    }
    for name, cmd in extras.items():
        run(cmd + [str(work / name)], check=True)

    spike = repo / "spikes" / "exif-writer" / "target" / "release" / "exif-writer-spike"
    log = []
    for source in ["rich-exif.jpg", "oversized-entry.jpg"]:
        result = run([str(spike), str(out), str(meta / source), *[str(work / n) for n in extras]], check=True)
        log.append(result.stdout.strip())

    cases = {
        ("rich-exif", False): expected["cases"][0],
        ("rich-exif", True): expected["cases"][1],
        ("oversized-entry", False): expected["cases"][2],
    }
    results = []
    for path in sorted(out.iterdir()):
        stem = path.name.split(".")[0]
        gps = stem.endswith("-gps")
        case = cases.get((stem.removesuffix("-gps"), gps))
        tags = exiftool_tags(path)
        entry = {
            "file": path.name,
            "bytes": path.stat().st_size,
            "exiftool_validate": validate(path),
            "exiv2_keys": len(exiv2_keys(path)),
            "pillow": pillow(path),
            "imageio": imageio(path),
            "imagemagick": magick(path),
            **container(path),
        }
        if case:
            mismatched, present = compare(tags, case)
            entry["value_mismatches"] = mismatched
            entry["forbidden_present"] = present
        results.append(entry)
    (work / "results.json").write_text(json.dumps({"spike_stdout": log, "results": results}, indent=2) + "\n")
    for r in results:
        print(
            f"{r['file']:40} validate={len(r['exiftool_validate'])} mismatches={len(r.get('value_mismatches', {}))}"
            f" forbidden={len(r.get('forbidden_present', []))} pillow={r['pillow'].get('orientation', r['pillow'].get('error'))}"
            f" imageio={r['imageio'].get('orientation')} magick={r['imagemagick']['exit']}"
        )


if __name__ == "__main__":
    main()
