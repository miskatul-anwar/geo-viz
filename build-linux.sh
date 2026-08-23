#!/usr/bin/env bash
# GeoViz Pro - Linux production build (Debian .deb / Fedora .rpm / AppImage)
#
# Prerequisites:
#   - .NET 8 SDK, Rust, Tauri CLI (`cargo install tauri-cli --version "^2.0" --locked`)
#   - libwebkit2gtk-4.1-dev, libgtk-3-dev, librsvg2-dev, patchelf, libsoup-3.0-dev
#
# Usage: ./build-linux.sh [bundle-list]   (default: "deb,rpm,appimage")
set -euo pipefail

ALL_BUNDLES="${1:-deb,rpm,appimage}"
VERSION="$(grep -oP '(?<="version": ")[^"]+' src-tauri/tauri.conf.json | head -1)"
BUNDLE_DIR="src-tauri/target/release/bundle"
export APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1

echo "========================================="
echo " Building GeoViz v${VERSION} for Linux (${ALL_BUNDLES})"
echo "========================================="

# Split requested bundles: pure-native bundles must succeed hard; a list
# containing appimage is allowed to fail because we have a robust fallback
# (some tauri-cli/linuxdeploy combos mis-stage binaries).
NATIVE_BUNDLES="$(printf '%s' "$ALL_BUNDLES" | sed 's/appimage,//g; s/,appimage//g')"
if [[ -n "$NATIVE_BUNDLES" ]]; then
    if [[ ",$ALL_BUNDLES," == *,appimage,* ]]; then
        cargo tauri build --bundles "$NATIVE_BUNDLES,appimage" || true
    else
        cargo tauri build --bundles "$NATIVE_BUNDLES"
    fi
fi

if [[ ",$ALL_BUNDLES," != *,appimage,* ]]; then
    exit 0
fi

APPIMAGE_PATH="$BUNDLE_DIR/appimage/geo-viz_${VERSION}_amd64.AppImage"
if [[ -f "$APPIMAGE_PATH" ]]; then
    echo "[ok] AppImage already present: $APPIMAGE_PATH"
    exit 0
fi

echo ""
echo "[fallback] Building AppImage via linuxdeploy..."

APPDIR="$PWD/$BUNDLE_DIR/appimage/geo-viz.AppDir"
CACHE="$HOME/.cache/tauri"
mkdir -p "$CACHE"

LINUXDEPLOY="$CACHE/linuxdeploy-x86_64.AppImage"
PLUGIN_APPIMAGE="$CACHE/linuxdeploy-plugin-appimage.AppImage"
if [[ ! -f "$LINUXDEPLOY" ]]; then
    curl -fsSL -o "$LINUXDEPLOY" \
        https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
    curl -fsSL -o "$PLUGIN_APPIMAGE" \
        https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage
    chmod +x "$LINUXDEPLOY" "$PLUGIN_APPIMAGE"
fi

cp src-tauri/target/release/geo-viz "$APPDIR/usr/bin/geo-viz"

# The GTK plugin enriches the bundle with schemas/modules but hard-fails on
# hosts without gdk-pixbuf dev files; core library deployment still happens
# without it, so treat it as best-effort.
"$LINUXDEPLOY" --appdir "$APPDIR" --plugin gtk \
    --desktop-file "$APPDIR/geo-viz.desktop" \
    --icon-file "$APPDIR/geo-viz.png" \
    --executable "$APPDIR/usr/bin/geo-viz" \
    --output appimage 2>&1 | grep -v "^WARNING" || \
    echo "[warn] linuxdeploy reported errors; verifying bundle contents..."

if ! [[ -f "$APPDIR/usr/bin/geo-viz" && -d "$APPDIR/usr/lib" ]]; then
    echo "error: linuxdeploy did not produce a usable AppDir" >&2
    exit 1
fi

# linuxdeploy's embedded packaging step can silently skip output; finish explicitly.
if ! compgen -G "$BUNDLE_DIR/appimage/*.AppImage" > /dev/null; then
    (cd "$BUNDLE_DIR/appimage" && ARCH=x86_64 "$PLUGIN_APPIMAGE" --appdir=geo-viz.AppDir)
fi

PRODUCED="$(compgen -G "$BUNDLE_DIR/appimage/geo-viz-*.AppImage" | head -1 || true)"
[[ -n "$PRODUCED" ]] && mv -f "$PRODUCED" "$APPIMAGE_PATH"

if [[ ! -f "$APPIMAGE_PATH" ]]; then
    echo "error: AppImage was not produced" >&2
    exit 1
fi

echo ""
echo "========================================="
echo " Build complete! Artifacts:"
find "$BUNDLE_DIR" -maxdepth 3 -type f \( \
    -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \) -printf '  -> %p\n'
echo "========================================="
