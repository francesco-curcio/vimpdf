#!/usr/bin/env bash
# Install vimpdf for the current user on Linux: the binary, a .desktop entry
# that declares it a PDF viewer (so it shows up in "Open With" and can be
# chosen by browsers), and its icon.
#
#   scripts/install-linux.sh             install into ~/.local
#   scripts/install-linux.sh --default   also make it the default PDF viewer
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release

bin_dir="$HOME/.local/bin"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
apps_dir="$data_dir/applications"
icon_dir="$data_dir/icons/hicolor/256x256/apps"
mkdir -p "$bin_dir" "$apps_dir" "$icon_dir"

install -m 755 target/release/vimpdf "$bin_dir/vimpdf"
install -m 644 assets/vimpdf-256.png "$icon_dir/vimpdf.png"

# Exec uses the full path: ~/.local/bin isn't always on the PATH of the
# graphical session. The file name must match the Wayland app id ("vimpdf").
cat > "$apps_dir/vimpdf.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=vimpdf
GenericName=PDF Viewer
Comment=Keyboard-driven, vim-style PDF viewer
Exec=$bin_dir/vimpdf %f
Icon=vimpdf
Terminal=false
Categories=Office;Viewer;
MimeType=application/pdf;
StartupWMClass=vimpdf
DESKTOP

command -v update-desktop-database >/dev/null && update-desktop-database -q "$apps_dir" || true
echo "installed $bin_dir/vimpdf and $apps_dir/vimpdf.desktop"

if [[ "${1:-}" == "--default" ]]; then
    xdg-mime default vimpdf.desktop application/pdf
    echo "vimpdf is now the default PDF viewer ($(xdg-mime query default application/pdf))"
fi
