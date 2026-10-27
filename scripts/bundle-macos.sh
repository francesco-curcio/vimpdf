#!/usr/bin/env bash
# Build target/vimpdf.app: a macOS app bundle that registers vimpdf as a PDF
# viewer, so it can be chosen in Finder's "Open With" and as Firefox's PDF
# handler.
#
#   scripts/bundle-macos.sh             build target/vimpdf.app
#   scripts/bundle-macos.sh --install   also copy it to ~/Applications
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=target/vimpdf.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/vimpdf "$app/Contents/MacOS/vimpdf"
cp assets/vimpdf.icns "$app/Contents/Resources/vimpdf.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>                 <string>vimpdf</string>
    <key>CFBundleDisplayName</key>          <string>vimpdf</string>
    <key>CFBundleIdentifier</key>           <string>local.vimpdf.viewer</string>
    <key>CFBundleExecutable</key>           <string>vimpdf</string>
    <key>CFBundleIconFile</key>             <string>vimpdf</string>
    <key>CFBundlePackageType</key>          <string>APPL</string>
    <key>CFBundleShortVersionString</key>   <string>${version}</string>
    <key>CFBundleVersion</key>              <string>${version}</string>
    <key>LSMinimumSystemVersion</key>       <string>11.0</string>
    <key>NSHighResolutionCapable</key>      <true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>     <string>PDF Document</string>
            <key>CFBundleTypeRole</key>     <string>Viewer</string>
            <key>LSHandlerRank</key>        <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array><string>com.adobe.pdf</string></array>
        </dict>
    </array>
</dict>
</plist>
PLIST

# Ad-hoc signature, so macOS runs the locally built bundle.
codesign --force --sign - "$app" >/dev/null 2>&1 || echo "warning: codesign failed"
echo "built $app"

if [[ "${1:-}" == "--install" ]]; then
    mkdir -p "$HOME/Applications"
    rm -rf "$HOME/Applications/vimpdf.app"
    cp -R "$app" "$HOME/Applications/vimpdf.app"
    # Make Launch Services notice the new app (and its PDF document type).
    lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
    [[ -x "$lsregister" ]] && "$lsregister" -f "$HOME/Applications/vimpdf.app"
    echo "installed ~/Applications/vimpdf.app"
fi
