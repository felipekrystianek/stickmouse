#!/bin/sh
# Builds Couchpad.app and Couchpad-<version>.dmg in target/release.
#
# The app is ad-hoc signed and not notarized, so on first open macOS asks the user to allow it in
# System Settings, Privacy & Security ("Open Anyway"). "Read Me First.txt" in the .dmg explains.
#
# The icon is committed as couchpad.icns; to redraw it, run make-icon.swift and convert the PNG
# with iconutil. To refresh THIRD_PARTY_LICENSES.html after changing dependencies, run
#   cargo about generate packaging/macos/licenses.hbs -o packaging/macos/THIRD_PARTY_LICENSES.html
set -eu
cd "$(dirname "$0")/../.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo build --release

app=target/release/Couchpad.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/padcursor "$app/Contents/MacOS/Couchpad"
cp packaging/macos/couchpad.icns "$app/Contents/Resources/couchpad.icns"
cp packaging/macos/THIRD_PARTY_LICENSES.html "$app/Contents/Resources/"
sed "s/VERSION/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
# Ad-hoc signature. macOS ties the Accessibility permission to it, so after an update users may
# need to switch Couchpad off and on again in the Accessibility list.
codesign --force --sign - "$app"
echo "Built $app"

dmg=target/release/Couchpad-$version.dmg
staging=target/release/dmg
rm -rf "$staging" "$dmg"
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
cp "packaging/macos/Read Me First.txt" "$staging/"
hdiutil create -quiet -volname Couchpad -srcfolder "$staging" -fs HFS+ -format UDZO "$dmg"
rm -rf "$staging"
echo "Built $dmg"

# The website serves the latest build and the license notices.
mkdir -p website/downloads
cp "$dmg" website/downloads/Couchpad.dmg
cp packaging/macos/THIRD_PARTY_LICENSES.html website/
echo "Copied to website/"
