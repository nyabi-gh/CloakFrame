#!/usr/bin/env python3
"""Lists the ftl messages whose English matches a source string of CloakFrame 1.12.1's Qt
translation, with 1.12.1's translation and the one the ftl file has now, so that a translation
still marked "## needs review" can take the wording 1.12.1 users already know.

    git show v1.12.1:translations/cloakframe_ja.ts > /tmp/ja.ts
    python3 -I tools/i18n/ts_matches.py crates/cloakframe-app/assets/i18n/en.ftl \
        crates/cloakframe-app/assets/i18n/ja.ftl /tmp/ja.ts

Only single-line messages are compared; the Qt strings and the ftl ones differ in placeholders
(%1 and { $name }), mnemonics (&) and ellipses, which are evened out before comparing.
"""

import re
import sys
import xml.etree.ElementTree as ET


def messages(path):
    """Each message id's single-line value and the "## " group it is under."""
    found = {}
    group = None
    with open(path, encoding="utf-8") as f:
        for line in f:
            if line.startswith("## "):
                group = line[3:].strip()
                continue
            match = re.match(r"^([a-z][a-z0-9-]*) = (.+)$", line.rstrip("\n"))
            if match:
                found[match.group(1)] = (match.group(2), group)
    return found


def normalized(text):
    text = text.replace("&&", "\0").replace("&", "").replace("\0", "&")
    text = text.replace("...", "…")
    text = re.sub(r"\{ *\$[a-z-]+ *\}|%[0-9n]", "#", text)
    text = re.sub(r"\s+", " ", text).strip().rstrip(".:").casefold()
    return text


def ts_translations(path):
    translated = {}
    for message in ET.parse(path).getroot().iter("message"):
        source = message.findtext("source")
        translation = message.find("translation")
        if source is None or translation is None:
            continue
        if translation.get("type") in ("unfinished", "obsolete", "vanished"):
            continue
        text = "".join(translation.itertext())
        if text:
            translated.setdefault(normalized(source), text)
    return translated


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    english, translated, ts = sys.argv[1:]
    english, translated, old = messages(english), messages(translated), ts_translations(ts)
    for id, (value, _) in english.items():
        before = old.get(normalized(value))
        if before is None:
            continue
        now, group = translated.get(id, (None, None))
        mark = "review" if group == "needs review" else "      "
        same = "=" if now is not None and normalized(now) == normalized(before) else "≠"
        print(f"{mark} {same} {id}\n    1.12.1: {before}\n    now:    {now}")


if __name__ == "__main__":
    main()
