#!/usr/bin/env bash
# M2 step 0: jpeg-encoder and libwebp (libwebp-sys) against cjpeg and cwebp at the D6 settings.
#   spikes/encode/run.sh <work dir>
# Needs libjpeg-turbo (cjpeg, djpeg), libwebp tools (cwebp, webpinfo), exiftool, ImageMagick and
# Python with Pillow.
set -euo pipefail

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
work=$(mkdir -p "$1" && cd "$1" && pwd)
assets="$work/assets"
out="$work/out"
mkdir -p "$assets" "$out"

python3 -I - "$repo/tests/fixtures/assets.json" "$assets" <<'EOF'
import hashlib, json, sys, urllib.request
from pathlib import Path
spec, dest = json.load(open(sys.argv[1])), Path(sys.argv[2])
for a in spec["images"]:
    p = dest / a["name"]
    if not p.exists():
        urllib.request.urlretrieve(a["url"], p)
    if hashlib.sha256(p.read_bytes()).hexdigest() != a["sha256"]:
        raise SystemExit(f"hash mismatch for {a['name']}")
EOF
# A 24 MP image for timing; content is the astronaut scaled up, so only speed is meaningful.
[ -f "$assets/large.png" ] || magick "$assets/astronaut.png" -filter Lanczos -resize 6000x4000! "$assets/large.png"

(cd "$repo/spikes/encode" && cargo build -q --release)
spike="$repo/spikes/encode/target/release/encode-spike"

echo "== source sampling (own SOF parser / Pillow / exiftool)"
for f in messi5.jpg lena.jpg; do
    pillow=$(python3 -I -c "from PIL import Image, JpegImagePlugin as J; print(J.get_sampling(Image.open('$assets/$f')))")
    exif=$(exiftool -s3 -YCbCrSubSampling "$assets/$f")
    echo "$f: $("$spike" sampling "$assets/$f") / $pillow / $exif"
done

echo "== JPEG q92"
echo "image sampling encoder bytes ms psnr_rgb psnr_y sampling_read(pillow) exiftool_warnings"
for f in astronaut.png messi5.jpg lena.jpg large.png; do
    stem=${f%.*}
    magick "$assets/$f" -depth 8 "ppm:$out/$stem.ppm"
    for s in 444 422 420; do
        ours="$out/$stem-$s-ours.jpg"
        r=$("$spike" jpeg "$assets/$f" "$ours" "$s")
        case $s in 444) cs=1x1 ;; 422) cs=2x1 ;; 420) cs=2x2 ;; esac
        turbo="$out/$stem-$s-cjpeg.jpg"
        t0=$(python3 -I -c 'import time; print(time.time())')
        cjpeg -quality 92 -sample "$cs" -optimize -outfile "$turbo" "$out/$stem.ppm"
        t1=$(python3 -I -c 'import time; print(time.time())')
        for enc in ours cjpeg; do
            file="$out/$stem-$s-$enc.jpg"
            p=$("$spike" psnr "$assets/$f" "$file")
            read_back=$(python3 -I -c "from PIL import Image, JpegImagePlugin as J; print(J.get_sampling(Image.open('$file')))")
            warn=$(exiftool -validate -warning -a -s3 "$file" | tr '\n' ' ')
            djpeg -outfile /dev/null "$file"
            if [ "$enc" = ours ]; then
                info=$(python3 -I -c "import json,sys; d=json.loads(sys.argv[1]); print(d['bytes'], round(d['ms'],1))" "$r")
            else
                info="$(stat -f%z "$file" 2>/dev/null || stat -c%s "$file") $(python3 -I -c "print(round(($t1-$t0)*1e3,1))")"
            fi
            psnr=$(python3 -I -c "import json,sys; d=json.loads(sys.argv[1]); print(round(d['psnr_rgb'],2), round(d['psnr_y'],2))" "$p")
            echo "$stem $s $enc $info $psnr $read_back ${warn:-none}"
        done
    done
done

echo "== JPEG with an ICC profile (Display P3 from macOS)"
icc=/System/Library/ColorSync/Profiles/Display\ P3.icc
if [ -f "$icc" ]; then
    "$spike" jpeg "$assets/astronaut.png" "$out/icc.jpg" 420 "$icc" > /dev/null
    echo "exiftool: $(exiftool -s3 -ProfileDescription "$out/icc.jpg"), magick: $(magick identify -format '%[icc:description]' "$out/icc.jpg")"
fi

echo "== lossy WebP q90"
echo "image alpha encoder bytes ms psnr_rgb psnr_y psnr_alpha webpinfo"
for f in astronaut.png car_image.webp large.png; do
    stem=${f%.*}
    for alpha in no alpha; do
        ours="$out/$stem-$alpha-ours.webp"
        ref="$assets/$f"
        if [ "$alpha" = alpha ]; then
            ref="$out/$stem-alpha-ref.png"
            magick "$assets/$f" -alpha set \( -size "$(magick identify -format '%wx%h' "$assets/$f")" gradient:white-black -rotate -90 \) -compose CopyOpacity -composite "$ref"
        fi
        r=$("$spike" webp "$ref" "$ours" "$alpha")
        turbo="$out/$stem-$alpha-cwebp.webp"
        t0=$(python3 -I -c 'import time; print(time.time())')
        cwebp -quiet -q 90 "$ref" -o "$turbo"
        t1=$(python3 -I -c 'import time; print(time.time())')
        for enc in ours cwebp; do
            file="$out/$stem-$alpha-$enc.webp"
            p=$("$spike" psnr "$ref" "$file")
            info=$(webpinfo "$file" 2>&1 | grep -E "Chunk (VP8 |VP8L|ALPH)" | awk '{print $2}' | tr -d ' ' | sort -u | tr '\n' '+')
            ok=$(webpinfo -quiet "$file" > /dev/null 2>&1 && echo valid || echo INVALID)
            if [ "$enc" = ours ]; then
                size=$(python3 -I -c "import json,sys; d=json.loads(sys.argv[1]); print(d['bytes'], round(d['ms'],1))" "$r")
            else
                size="$(stat -f%z "$file" 2>/dev/null || stat -c%s "$file") $(python3 -I -c "print(round(($t1-$t0)*1e3,1))")"
            fi
            psnr=$(python3 -I -c "import json,sys; d=json.loads(sys.argv[1]); r=lambda v: 'inf' if v is None else round(v,2); print(r(d['psnr_rgb']), r(d['psnr_y']), r(d['psnr_alpha']))" "$p")
            echo "$stem $alpha $enc $size $psnr $ok:${info%+}"
        done
    done
done
