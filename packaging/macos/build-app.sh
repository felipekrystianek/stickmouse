#!/bin/sh
# Builds Stickmouse.app and Stickmouse-<version>.dmg in target/release.
#
# The app is ad-hoc signed and not notarized, so on first open macOS asks the user to allow it in
# System Settings, Privacy & Security ("Open Anyway"). "Read Me First.txt" in the .dmg explains.
#
# The icon is committed as stickmouse.icns; to redraw it, run make-icon.swift and convert the PNG
# with iconutil. To refresh THIRD_PARTY_LICENSES.html after changing dependencies, run
#   cargo about generate packaging/macos/licenses.hbs -o packaging/macos/THIRD_PARTY_LICENSES.html
set -eu
cd "$(dirname "$0")/../.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo build --release

app=target/release/Stickmouse.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/stickmouse "$app/Contents/MacOS/Stickmouse"
cp packaging/macos/stickmouse.icns "$app/Contents/Resources/stickmouse.icns"
cp packaging/macos/THIRD_PARTY_LICENSES.html "$app/Contents/Resources/"
sed "s/VERSION/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
# macOS ties the Accessibility permission to the signature. Signing every build with the same
# certificate keeps the permission across updates; an ad-hoc signature changes with each build.
# The "Stickmouse Developer" certificate is self-signed and lives in the maintainer's login
# keychain. Keep a backup of it: a new certificate means every user grants access again.
identity=$(security find-identity -p codesigning | sed -n 's/.*) \([0-9A-F]\{40\}\) "Stickmouse Developer".*/\1/p' | head -n 1)
if [ -n "$identity" ]; then
    codesign --force --sign "$identity" "$app"
else
    echo "warning: no \"Stickmouse Developer\" certificate, signing ad hoc" >&2
    codesign --force --sign - "$app"
fi
echo "Built $app"

dmg=target/release/Stickmouse-$version.dmg
staging=target/release/dmg
rm -rf "$staging" "$dmg"
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
cp "packaging/macos/Read Me First.txt" "$staging/"
hdiutil create -quiet -volname Stickmouse -srcfolder "$staging" -fs HFS+ -format UDZO "$dmg"
rm -rf "$staging"
echo "Built $dmg"

# The website serves the latest build and the license notices.
mkdir -p website/downloads
cp "$dmg" website/downloads/Stickmouse.dmg
cp packaging/macos/THIRD_PARTY_LICENSES.html website/
echo "Copied to website/"
