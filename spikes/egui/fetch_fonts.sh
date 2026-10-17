#!/usr/bin/env bash
# Fetches Noto Sans KR/JP (SIL OFL 1.1) at a fixed tag and subsets them for the spike.
# Needs pyftsubset (brew install fonttools). Output: assets/fonts/ (not committed).
set -euo pipefail
cd "$(dirname "$0")"
out=assets/fonts
work=$(mktemp -d)
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
# Every modern Hangul syllable plus both jamo blocks: IME preedit shows lone jamo.
pyftsubset "$work/kr.otf" --unicodes="$common,U+1100-11FF,U+3130-318F,U+AC00-D7A3" \
    --layout-features='*' --output-file="$out/NotoSansKR-Subset.otf"
# Kana and the JIS X 0208 kanji (levels 1 and 2), the set Japanese IMEs produce by default.
python3 -I -c '
import sys
chars = set()
for row in range(16, 85):
    for cell in range(1, 95):
        try:
            chars.add(bytes([0xA0 + row, 0xA0 + cell]).decode("euc_jp"))
        except UnicodeDecodeError:
            pass
sys.stdout.write("".join(sorted(chars)))
' > "$work/jis.txt"
pyftsubset "$work/jp.otf" --unicodes="$common,U+3040-30FF,U+31F0-31FF" --text-file="$work/jis.txt" \
    --layout-features='*' --output-file="$out/NotoSansJP-Subset.otf"
ls -l "$out"
