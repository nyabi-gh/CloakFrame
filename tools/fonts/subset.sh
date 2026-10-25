#!/usr/bin/env bash
# Makes the app's bundled Noto Sans KR/JP subsets (SIL OFL 1.1) from a fixed noto-cjk tag.
# Needs pyftsubset (fonttools). Output: crates/cloakframe-app/assets/fonts/, which is committed.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
out=$root/crates/cloakframe-app/assets/fonts
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"
base=https://github.com/notofonts/noto-cjk/raw/Sans2.004
if ! command -v sha256sum >/dev/null 2>&1; then
    sha256sum() { shasum -a 256 "$@"; }
fi
fetch() {
    curl -fsSL --retry 3 "$base/$1" -o "$work/$2"
    echo "$3  $work/$2" | sha256sum -c -
}
fetch Sans/SubsetOTF/KR/NotoSansKR-Regular.otf kr.otf 69975a0ac8472717870aefeab0a4d52739308d90856b9955313b2ad5e0148d68
fetch Sans/SubsetOTF/JP/NotoSansJP-Regular.otf jp.otf dff723ba59d57d136764a04b9b2d03205544f7cd785a711442d6d2d085ac5073
fetch LICENSE OFL.txt 6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2
cp "$work/OFL.txt" "$out/OFL.txt"

common="U+0020-007E,U+00A0-00FF,U+2000-206F,U+2190-21FF,U+2460-24FF,U+25A0-25FF,U+3000-303F,U+FF00-FFEF"
# Every character of a character set, from Python's codec for it.
charset() {
    python3 -I -c '
import itertools, sys
codec = sys.argv[1]
if codec == "cp932":
    leads = [bytes([b]) for b in list(range(0x81, 0xA0)) + list(range(0xE0, 0xFD))]
    trails = [bytes([b]) for b in list(range(0x40, 0x7F)) + list(range(0x80, 0xFD))]
    seqs = [a + b for a, b in itertools.product(leads, trails)]
else:
    leads = [bytes([b]) for b in range(0xA1, 0xFF)]
    seqs = [a + b for a, b in itertools.product(leads, leads)]
if codec == "euc_jis_2004":
    seqs += [b"\x8f" + s for s in seqs]
chars = set()
for s in seqs:
    try:
        text = s.decode(codec)
    except UnicodeDecodeError:
        continue
    chars.update(c for c in text if ord(c) > 0x7F)
sys.stdout.write("".join(sorted(chars)))
' "$1"
}
# Hangul syllables and both jamo blocks (an IME's preedit shows lone jamo), and the KS X 1001
# hanja that Korean names use.
charset euc_kr > "$work/ksx1001.txt"
pyftsubset "$work/kr.otf" --unicodes="$common,U+1100-11FF,U+3130-318F,U+AC00-D7A3" \
    --text-file="$work/ksx1001.txt" --layout-features='*' --output-file="$out/NotoSansKR-Subset.otf"
# Kana, all of JIS X 0213 and the NEC and IBM extensions of Windows' cp932: names use variants
# JIS X 0208 lacks (﨑 and 德 are in JIS X 0213, 髙 only in the IBM extension).
{ charset euc_jis_2004; charset cp932; } > "$work/jisx0213.txt"
pyftsubset "$work/jp.otf" --unicodes="$common,U+3040-30FF,U+31F0-31FF" \
    --text-file="$work/jisx0213.txt" --layout-features='*' --output-file="$out/NotoSansJP-Subset.otf"
(cd "$out" && sha256sum NotoSansKR-Subset.otf NotoSansJP-Subset.otf OFL.txt)
ls -l "$out"
