#!/usr/bin/env bash
# Regenerate assets/vimpdf.icns from scripts/make-icon.swift (macOS only).
set -euo pipefail
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
swift scripts/make-icon.swift "$tmp/icon.png"
iconset="$tmp/vimpdf.iconset"
mkdir "$iconset"
for s in 16 32 128 256 512; do
    sips -z $s $s "$tmp/icon.png" --out "$iconset/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) "$tmp/icon.png" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o assets/vimpdf.icns
cp "$tmp/icon.png" assets/vimpdf.png
rm -rf "$tmp"
echo "wrote assets/vimpdf.icns"
