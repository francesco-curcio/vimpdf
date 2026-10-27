#!/usr/bin/env bash
# Regenerate assets/pdfv.icns from scripts/make-icon.swift (macOS only).
set -euo pipefail
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
swift scripts/make-icon.swift "$tmp/icon.png"
iconset="$tmp/pdfv.iconset"
mkdir "$iconset"
for s in 16 32 128 256 512; do
    sips -z $s $s "$tmp/icon.png" --out "$iconset/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) "$tmp/icon.png" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o assets/pdfv.icns
cp "$tmp/icon.png" assets/pdfv.png
rm -rf "$tmp"
echo "wrote assets/pdfv.icns"
