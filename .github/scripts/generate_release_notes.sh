#!/usr/bin/env bash
set -euo pipefail

# Writes the release notes for one version: its "## <version>" section of CHANGELOG.md, followed
# by a compare link to the previous release tag. Fails when the section is missing or empty.
#
# The compare link needs the full history and tags (actions/checkout with fetch-depth: 0) and a
# v<version> tag; without them the notes are the section alone.
#
# Usage: generate_release_notes.sh <version> <output-file>

version="${1:?usage: generate_release_notes.sh <version> <output-file>}"
output="${2:?usage: generate_release_notes.sh <version> <output-file>}"
repository="${GITHUB_REPOSITORY:-nyabi-gh/CloakFrame}"
changelog="$(git rev-parse --show-toplevel)/CHANGELOG.md"

section="$(awk -v heading="## ${version}" '
    { sub(/\r$/, "") }
    /^## / { if (found) exit; found = ($0 == heading); next }
    found
' "$changelog" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')"

if [[ -z "${section//[[:space:]]/}" ]]; then
    echo "CHANGELOG.md has no notes under '## ${version}'." >&2
    exit 1
fi

head="v${version}"
previous=""
if git rev-parse -q --verify "refs/tags/${head}" >/dev/null; then
    previous="$(git describe --tags --abbrev=0 --match 'v[0-9]*.[0-9]*.[0-9]*' "${head}^" 2>/dev/null || true)"
fi

{
    printf '%s\n' "$section"
    if [[ -n "$previous" ]]; then
        echo
        echo "**Full changelog:** https://github.com/${repository}/compare/${previous}...${head}"
    fi
} > "$output"
