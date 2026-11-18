#!/bin/sh
# Print the CHANGELOG section for a release, for its notes.
#
#   scripts/release-notes.sh v0.1.0
#   scripts/release-notes.sh 0.1.0
#
# Nothing is printed when the section is missing, so the caller decides what to
# say instead: a release with no notes is a release nobody can read.
set -eu

version=${1:?usage: scripts/release-notes.sh vX.Y.Z}
version=${version#v}

awk -v version="$version" '
  index($0, "## [" version "]") == 1 { printing = 1; next }
  printing && /^## / { exit }
  printing { print }
' "$(dirname "$0")/../CHANGELOG.md" | sed -e '/./,$!d'
