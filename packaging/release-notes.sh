#!/usr/bin/env sh
# Prints the GitHub release notes for VERSION (e.g. 0.3.0): its section of
# CHANGELOG.md, how to install it, and a link to every commit since the
# previous release. Fails if CHANGELOG.md has no section for VERSION, so no
# release goes out without one.
#
#   packaging/release-notes.sh VERSION
set -eu

cd "$(dirname "$0")/.."
version=$1

# From "## VERSION – date" up to the next "## " heading, without the blank
# lines around it.
notes=$(awk -v v="$version" '/^## / { if (found) exit; found = ($2 == v); next } found' CHANGELOG.md | sed '/./,$!d')
if [ -z "$notes" ]; then
    echo "CHANGELOG.md has no section for $version" >&2
    exit 1
fi

printf "## What's Changed\n\n%s\n\n" "$notes"
sed "s/@VERSION@/v$version/g" packaging/release-install.md

previous=$(git describe --tags --abbrev=0 --exclude "v$version" HEAD 2>/dev/null || true)
if [ -n "$previous" ]; then
    printf '\n**Full changelog**: https://github.com/Ikuu/pullbox/compare/%s...v%s\n' "$previous" "$version"
fi
