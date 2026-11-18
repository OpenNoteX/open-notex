#!/usr/bin/env bash
# Build assets/icon.icns from assets/icon.svg.
#
# The SVG is the source: Quick Look renders it, `sips` makes the sizes and
# `iconutil` packs the .iconset. Nothing is exported by hand, so the icon in the
# bundle cannot drift from the one in the repository.
#
# Run from anywhere: scripts/make-icon.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
svg="$root/assets/icon.svg"
icns="$root/assets/icon.icns"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

qlmanage -t -s 1024 -o "$tmp" "$svg" >/dev/null 2>&1
png="$tmp/icon.svg.png"
if [ ! -f "$png" ]; then
  echo "make-icon: Quick Look produced no image for $svg" >&2
  exit 1
fi

iconset="$tmp/icon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done

iconutil -c icns "$iconset" -o "$icns"
echo "wrote $icns ($(wc -c <"$icns" | tr -d ' ') bytes)"
