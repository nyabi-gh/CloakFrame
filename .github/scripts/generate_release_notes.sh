#!/usr/bin/env bash
set -euo pipefail

# Writes the release notes for one version: the subject of every commit since the previous
# release tag, newest first. Commits that only prepare a release ("Prepare 1.2.3") are left out.
#
# Needs the full history and tags (actions/checkout with fetch-depth: 0). Without a v<version>
# tag, as in a workflow_dispatch run, the notes cover the commits up to HEAD.
#
# Usage: generate_release_notes.sh <version> <output-file>

version="${1:?usage: generate_release_notes.sh <version> <output-file>}"
output="${2:?usage: generate_release_notes.sh <version> <output-file>}"
repository="${GITHUB_REPOSITORY:-nyabi-gh/CloakFrame}"

head="v${version}"
tagged=true
if ! git rev-parse -q --verify "refs/tags/${head}" >/dev/null; then
    head=HEAD
    tagged=false
fi
previous="$(git describe --tags --abbrev=0 --match 'v[0-9]*.[0-9]*.[0-9]*' "${head}^" 2>/dev/null || true)"

subjects="$(git log --no-merges --format='%s' "${previous:+${previous}..}${head}")"
changes="$(grep -v -E '^Prepare [0-9]+\.[0-9]+\.[0-9]+$' <<<"$subjects" | sed '/^$/d; s/^/- /' || true)"

{
    if [[ -n "$changes" ]]; then
        printf '%s\n' "$changes"
    else
        echo "- Maintenance release with no recorded changes."
    fi
    if [[ -n "$previous" && "$tagged" == true ]]; then
        echo
        echo "**Full changelog:** https://github.com/${repository}/compare/${previous}...${head}"
    fi
} > "$output"
