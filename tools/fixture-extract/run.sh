#!/usr/bin/env bash
# Regenerates tests/fixtures from the v1.12.1 tag.
#
# Needs macOS with the 1.12.1 Homebrew dependencies plus exiv2, exiftool, imagemagick and
# python3 with numpy and Pillow. The legacy sources are exported to a work directory and only
# that copy is modified.
#
#   tools/fixture-extract/run.sh [work dir]
set -euo pipefail

repo=$(git rev-parse --show-toplevel)
here="$repo/tools/fixture-extract"
fixtures="$repo/tests/fixtures"
work=${1:-$(mktemp -d)}
mkdir -p "$work"
work=$(cd "$work" && pwd)
echo "work directory: $work"

if ! command -v sha256sum >/dev/null 2>&1; then
    sha256sum() { shasum -a 256 "$@"; }
fi

RUNNER_TEMP="$work" GITHUB_ENV="$work/models.env" bash "$repo/.github/scripts/fetch_test_models.sh"
assets="$work/cloakframe-test-assets"
fetch() {
    local url="$1" name="$2" sha256="$3"
    [ -f "$assets/$name" ] || curl -fsSL --retry 3 "$url" -o "$assets/$name"
    echo "$sha256  $assets/$name" | sha256sum -c -
}
while IFS=$'\t' read -r name url sha256; do
    fetch "$url" "$name" "$sha256"
done < <(python3 -I -c '
import json, sys
for a in json.load(open(sys.argv[1]))["images"]:
    print(a["name"], a["url"], a["sha256"], sep="\t")
' "$fixtures/assets.json")

rm -rf "$work/src" "$work/build"
mkdir -p "$work/src"
git -C "$repo" archive v1.12.1 | tar -x -C "$work/src"
python3 -I "$here/transform.py" "$work/src/tests"
echo "add_subdirectory(\"$here\" fixture-extract)" >> "$work/src/CMakeLists.txt"
cmake -S "$work/src" -B "$work/build" -G Ninja -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON \
    -DCLOAKFRAME_SELF_UPDATE=OFF -DCMAKE_CXX_FLAGS="-I$here" > "$work/configure.log"
grep -q "metadata preservation enabled" "$work/configure.log" || {
    echo "Exiv2 was not found; metadata fixtures need it (brew install exiv2)" >&2
    exit 1
}
cmake --build "$work/build" --parallel > "$work/build.log"

python3 -I "$here/gen_inputs.py" "$fixtures" "$assets/astronaut.png" "$work/gen"

set -a
# shellcheck disable=SC1091
source "$work/models.env"
set +a
rm -rf "$work/legacy"
CFREC_OUT="$work/legacy" ctest --test-dir "$work/build" --parallel 8 --output-on-failure > "$work/ctest.log"
rm -rf "$fixtures/legacy/"*/
for suite in "$work/legacy"/*/; do
    name=$(basename "$suite")
    case "$name" in
        custom_model_consent_tests | stage_cleanup_tests | update_cache_tests) continue ;;
    esac
    mkdir -p "$fixtures/legacy"
    cp -R "$suite" "$fixtures/legacy/$name"
done
# Removed features keep their index entry but not their recorded data.
python3 -I -c '
import json, shutil, sys
from pathlib import Path
legacy = Path(sys.argv[1])
for key, entry in json.loads((legacy / "index.json").read_text())["tests"].items():
    if entry["status"] == "dropped":
        (legacy / f"{key}.json").unlink(missing_ok=True)
        shutil.rmtree(legacy / key, ignore_errors=True)
' "$fixtures/legacy"

rm -rf "$fixtures/detect/yolo5face" "$fixtures/detect/yunet" "$fixtures/detect/plate"
images=()
for name in astronaut.png messi5.jpg lena.jpg car_image.webp; do
    images+=("$assets/$name")
done
images+=("$fixtures/detect/inputs/astronaut-scales.png" "$fixtures/detect/inputs/crowd-280-faces.png")
"$work/build/fixture-extract/cloakframe_fixture_detect" "$assets" "$fixtures/detect" "${images[@]}"

"$work/build/fixture-extract/cloakframe_fixture_defects" "$fixtures" "$assets" "$work/defects"
python3 -I "$here/analyze.py" "$fixtures" "$work/defects"
echo "fixtures written to $fixtures"
