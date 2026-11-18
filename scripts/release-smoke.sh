#!/usr/bin/env bash
# Prove the file people download works: mount the disk image, copy the app the
# way a drag to Applications does, launch it with a home of its own, and wait
# for Rune to say its first window is up.
#
#   scripts/release-smoke.sh artifacts/dist/openNoteX-0.1.0-arm64.dmg
#
# `cargo xtask bundle --check` proves the build tree launches. This proves the
# artifact does, after a mount and a copy.
set -euo pipefail

image=${1:?usage: scripts/release-smoke.sh path/to/openNoteX-<version>-arm64.dmg}
image=$(cd "$(dirname "$image")" && pwd)/$(basename "$image")
[ -f "$image" ] || { echo "no disk image at $image" >&2; exit 1; }

tmp=$(mktemp -d)
mounted=0
cleanup() {
  if [ "$mounted" = 1 ]; then
    hdiutil detach "$tmp/vol" >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT

mkdir -p "$tmp/vol" "$tmp/home" "$tmp/vault" "$tmp/ready" "$tmp/Applications"
# hdiutil is deprecated in favour of `diskutil image attach`, which older
# macOS lacks; this runs on the runner, so keep the one that is everywhere.
if ! hdiutil attach -nobrowse -readonly -mountpoint "$tmp/vol" "$image" \
  >/dev/null 2>"$tmp/attach.log"; then
  cat "$tmp/attach.log" >&2
  exit 1
fi
mounted=1

# The image's one job is to be draggable onto Applications.
if [ ! -L "$tmp/vol/Applications" ]; then
  echo "the image has no Applications link" >&2
  exit 1
fi
app="$tmp/vol/openNoteX.app"
[ -d "$app" ] || { echo "the image has no openNoteX.app" >&2; exit 1; }

ditto "$app" "$tmp/Applications/openNoteX.app"
installed="$tmp/Applications/openNoteX.app"

# The bundle macOS will see: its own identity, not the file's name.
identifier=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" \
  "$installed/Contents/Info.plist")
if [ "$identifier" != "com.opennote.opennotex" ]; then
  echo "the bundle id is $identifier" >&2
  exit 1
fi

HOME="$tmp/home" RUNE_READY_DIR="$tmp/ready" \
  "$installed/Contents/MacOS/open-notex" --vault "$tmp/vault" \
  >"$tmp/app.log" 2>&1 &
pid=$!

receipt=""
for _ in $(seq 1 400); do
  receipt=$(find "$tmp/ready" -type f -name "com.opennote.opennotex.*" 2>/dev/null | head -1)
  [ -n "$receipt" ] && break
  sleep 0.05
done

kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true

if [ -z "$receipt" ]; then
  echo "the installed app never reported a ready window" >&2
  cat "$tmp/app.log" >&2 || true
  exit 1
fi
grep -q '"bundle_id":"com.opennote.opennotex"' "$receipt" || {
  echo "the ready receipt does not name the bundle: $(cat "$receipt")" >&2
  exit 1
}

echo "the disk image installs and launches: $(cat "$receipt")"
