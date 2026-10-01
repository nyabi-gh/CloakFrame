from __future__ import annotations

import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

PLACEHOLDER = re.compile(r"%(?:\d+|n)")
ALLOW_SAME = {
    "FFmpeg", "H.264", "HEVC", "NMS", "ONNX", "CloakFrame",
    "%1 / %2",
}


def text(element: ET.Element | None) -> str:
    return "" if element is None else "".join(element.itertext()).strip()


def check(path: Path, strict_source_equality: bool) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    warnings: list[str] = []
    root = ET.parse(path).getroot()
    seen: set[tuple[str, str]] = set()

    for context in root.findall("context"):
        context_name = text(context.find("name"))
        for message in context.findall("message"):
            source = text(message.find("source"))
            key = (context_name, source)
            if key in seen:
                errors.append(f"{context_name}: duplicate source: {source!r}")
            seen.add(key)

            translation = message.find("translation")
            # lupdate marks an entry vanished when no scanned source uses it any more, and
            # lrelease leaves it out. If the source string still exists, that string now shows
            # in English; if it does not, the entry is dead. Either way it must not stay.
            if translation is not None and translation.get("type") in ("vanished", "obsolete"):
                errors.append(f"{context_name}: vanished: {source!r}")
                continue
            if translation is None or translation.get("type") == "unfinished":
                errors.append(f"{context_name}: unfinished: {source!r}")
                continue

            forms = translation.findall("numerusform")
            rendered = [text(form) for form in forms] if forms else [text(translation)]
            if any(not value for value in rendered):
                errors.append(f"{context_name}: empty translation: {source!r}")
                continue

            expected = sorted(PLACEHOLDER.findall(source))
            for value in rendered:
                actual = sorted(PLACEHOLDER.findall(value))
                if actual != expected:
                    errors.append(
                        f"{context_name}: placeholders {actual} != {expected}: {source!r}"
                    )
                if value == source and source not in ALLOW_SAME:
                    issue = f"{context_name}: translation equals source: {source!r}"
                    (errors if strict_source_equality else warnings).append(issue)

    return errors, warnings


def main() -> int:
    strict_source_equality = "--strict-source-equality" in sys.argv
    catalogs = [
        Path(arg) for arg in sys.argv[1:]
        if arg != "--strict-source-equality"
    ]
    if not catalogs:
        catalogs = sorted(Path("translations").glob("cloakframe_*.ts"))
    baseline: set[tuple[str, str]] | None = None
    failed = False

    for catalog in catalogs:
        errors, warnings = check(catalog, strict_source_equality)
        root = ET.parse(catalog).getroot()
        keys = {
            (text(context.find("name")), text(message.find("source")))
            for context in root.findall("context")
            for message in context.findall("message")
        }
        if baseline is None:
            baseline = keys
        elif keys != baseline:
            missing = len(baseline - keys)
            extra = len(keys - baseline)
            errors.append(f"catalog differs from baseline: {missing} missing, {extra} extra")

        if errors:
            failed = True
            print(f"{catalog}: {len(errors)} error(s)", file=sys.stderr)
            for error in errors:
                print(f"  {error}", file=sys.stderr)
        else:
            suffix = f", {len(warnings)} source-equality warning(s)" if warnings else ""
            print(f"{catalog}: {len(keys)} messages OK{suffix}")

    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
