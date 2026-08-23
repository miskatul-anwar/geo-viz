#!/usr/bin/env bash
# GeoViz Pro - macOS build (.app bundle + .dmg installer)
# Prerequisites: .NET 8 SDK, Rust, Tauri CLI (`cargo install tauri-cli --version "^2.0" --locked`)
# For a universal binary (Intel + Apple Silicon), both Rust targets must be installed:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
set -euo pipefail

TARGET_ARG=""
if rustup target list --installed | grep -q x86_64-apple-darwin && \
   rustup target list --installed | grep -q aarch64-apple-darwin; then
    echo "Both macOS targets present -> building universal binary."
    TARGET_ARG="--target universal-apple-darwin"
fi

echo "========================================="
echo " Building GeoViz for macOS"
echo "========================================="

# The frontend publish runs automatically via beforeBuildCommand in tauri.conf.json.
cargo tauri build --bundles dmg,app $TARGET_ARG

echo ""
echo "========================================="
echo " Build complete! Artifacts:"
find src-tauri/target -path '*release/bundle/dmg/*.dmg' -o -path '*release/bundle/macos/*.app' |
    while read -r f; do echo "  -> $f"; done
echo "========================================="
