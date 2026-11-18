#!/bin/sh
# Install or update openNoteX from a GitHub release.
#
#   curl -fsSLO https://raw.githubusercontent.com/OpenNoteX/open-notex/main/scripts/install.sh
#   sh install.sh            # the latest release
#   sh install.sh 0.1.0      # a specific one
#
# It downloads the zip, checks its SHA-256 against the release's SHA256SUMS.txt,
# installs to ~/Applications/openNoteX.app (or /Applications when that is
# writable), clears the quarantine flag macOS puts on a download, and opens the
# app. Without an Apple Developer account the app is ad-hoc signed, so this is
# the one path that does not ask a person to find System Settings first.
#
# `NO_OPEN=1` installs without launching, `NOTEX_APP_DIR` chooses where the app
# lands, and `NOTEX_BASE_URL` points at a folder or mirror instead of the
# release download. The last two exist so the installer can be tested without a
# published release.
set -eu

REPO=OpenNoteX/open-notex
APP=openNoteX.app
ID=com.opennote.opennotex

say() { printf '%s\n' "$*"; }
die() {
  printf 'install: %s\n' "$*" >&2
  exit 1
}

command -v curl >/dev/null || die "curl is missing"
command -v ditto >/dev/null || die "ditto is missing"
[ "$(uname -s)" = Darwin ] || die "this installer is for macOS"

version=${1:-}
if [ -z "$version" ]; then
  # The release API, once: the tag is the only thing needed from it.
  latest=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null) ||
    die "could not ask GitHub for the latest release; pass a version instead, e.g. sh install.sh 0.1.0"
  version=$(printf '%s' "$latest" | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -1)
  [ -n "$version" ] || die "no published release found; pass a version, e.g. sh install.sh 0.1.0"
fi
version=${version#v}

zip="openNoteX-$version-arm64.zip"
base=${NOTEX_BASE_URL:-"https://github.com/$REPO/releases/download/v$version"}

tmp=$(mktemp -d)
cleanup() {
  # A failed swap leaves the old bundle in `$tmp/old`; put it back before the
  # directory goes, so an interrupted install is never a missing app.
  if [ -d "$tmp/old/$APP" ] && [ ! -e "$tmp/installed" ]; then
    dest=$(cat "$tmp/dest" 2>/dev/null || true)
    if [ -n "$dest" ] && [ ! -e "$dest" ]; then
      mkdir -p "$(dirname "$dest")"
      mv "$tmp/old/$APP" "$dest" 2>/dev/null || true
      say "install: restored the previous $APP"
    fi
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT HUP INT TERM

say "install: openNoteX $version"
curl -fsSL -o "$tmp/$zip" "$base/$zip" || die "could not download $base/$zip"
curl -fsSL -o "$tmp/SHA256SUMS.txt" "$base/SHA256SUMS.txt" || die "could not download the checksums"

expected=$(awk -v name="$zip" '$2 == name { print $1 }' "$tmp/SHA256SUMS.txt")
[ -n "$expected" ] || die "the release does not list $zip"
actual=$(shasum -a 256 "$tmp/$zip" | cut -d ' ' -f 1)
[ "$expected" = "$actual" ] || die "the download does not match its checksum"

ditto -x -k "$tmp/$zip" "$tmp/unpack" || die "could not unpack $zip"
[ -d "$tmp/unpack/$APP" ] || die "the archive does not contain $APP"

if [ -n "${NOTEX_APP_DIR:-}" ]; then
  mkdir -p "$NOTEX_APP_DIR"
  dest="$NOTEX_APP_DIR/$APP"
elif [ -w /Applications ]; then
  dest=/Applications/$APP
else
  mkdir -p "$HOME/Applications"
  dest="$HOME/Applications/$APP"
fi
printf '%s' "$dest" >"$tmp/dest"

# Ask a running copy to quit; replacing a bundle underneath it would leave the
# process reading files that no longer exist.
if pgrep -f "$dest/Contents/MacOS/" >/dev/null 2>&1; then
  say "install: quitting the openNoteX that is running"
  osascript -e "tell application id \"$ID\" to quit" >/dev/null 2>&1 || true
  i=0
  while pgrep -f "$dest/Contents/MacOS/" >/dev/null 2>&1; do
    i=$((i + 1))
    [ "$i" -gt 100 ] && die "openNoteX is still running; quit it and run this again"
    sleep 0.1
  done
fi

# Stage beside the destination, then swap with two renames on one volume, so the
# window with no bundle is microseconds rather than a copy.
ditto "$tmp/unpack/$APP" "$dest.new" || die "could not stage $APP"
mkdir -p "$tmp/old"
if [ -e "$dest" ]; then
  mv "$dest" "$tmp/old/$APP" || die "could not move the old $APP aside"
fi
if ! mv "$dest.new" "$dest"; then
  [ -e "$tmp/old/$APP" ] && mv "$tmp/old/$APP" "$dest"
  die "could not install $APP"
fi
rm -rf "$tmp/old"
: >"$tmp/installed"

# The download carries a quarantine flag; an ad-hoc signed app then needs a
# System Settings detour that the installer exists to avoid. This clears the
# flag the person already chose to accept by running the script.
xattr -dr com.apple.quarantine "$dest" 2>/dev/null || true

say "install: $("$dest/Contents/MacOS/open-notex" --version) in $dest"

if [ "${NO_OPEN:-0}" = "1" ]; then
  exit 0
fi
open "$dest"
