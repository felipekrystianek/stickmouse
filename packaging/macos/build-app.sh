#!/bin/sh
# Builds padcursor.app in target/release. Copy it to /Applications, open it once and allow it in
# System Settings, Privacy & Security, Accessibility. Add it under General, Login Items to start
# it with the Mac.
set -eu
cd "$(dirname "$0")/../.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo build --release

app=target/release/padcursor.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp target/release/padcursor "$app/Contents/MacOS/padcursor"
sed "s/VERSION/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
# Ad-hoc signature. macOS ties the Accessibility permission to it, so after rebuilding you may
# need to remove padcursor from the Accessibility list and allow it again.
codesign --force --sign - "$app"
echo "Built $app"
