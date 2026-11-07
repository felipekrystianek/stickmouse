#!/bin/sh
# Builds Stickmouse.app and Stickmouse-<version>.dmg in target/release.
#
# The app is not notarized, so on first open macOS asks the user to allow it in
# System Settings, Privacy & Security ("Open Anyway"). "Read Me First.txt" in the .dmg explains.
#
# The icon is committed as stickmouse.icns; to redraw it, run make-icon.swift and convert the PNG
# with iconutil. To refresh THIRD_PARTY_LICENSES.html after changing dependencies, run
#   cargo about generate packaging/macos/licenses.hbs -o packaging/macos/THIRD_PARTY_LICENSES.html
set -eu
cd "$(dirname "$0")/../.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo build --release

# Sparkle does the updates (src/macos/updater.rs). The release is fetched once and checked
# against a fixed hash; its bin/ tools sign each update with the EdDSA key in the keychain.
sparkle_version=2.10.1
sparkle_sha256=1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27
sparkle=target/sparkle-$sparkle_version
if [ ! -d "$sparkle/Sparkle.framework" ]; then
    rm -rf "$sparkle" && mkdir -p "$sparkle"
    curl -fsSL -o "$sparkle.tar.xz" \
        "https://github.com/sparkle-project/Sparkle/releases/download/$sparkle_version/Sparkle-$sparkle_version.tar.xz"
    echo "$sparkle_sha256  $sparkle.tar.xz" | shasum -a 256 -c -
    tar -xf "$sparkle.tar.xz" -C "$sparkle"
    rm "$sparkle.tar.xz"
fi

app=target/release/Stickmouse.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Frameworks"
ditto "$sparkle/Sparkle.framework" "$app/Contents/Frameworks/Sparkle.framework"
cp target/release/stickmouse "$app/Contents/MacOS/Stickmouse"
cp packaging/macos/stickmouse.icns "$app/Contents/Resources/stickmouse.icns"
cp packaging/macos/THIRD_PARTY_LICENSES.html "$app/Contents/Resources/"
sed "s/VERSION/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
# macOS ties the Accessibility permission to the signature. Signing every build with the same
# certificate keeps the permission across updates; an ad-hoc signature changes with each build.
# The "Stickmouse Developer" certificate is self-signed and lives in the maintainer's login
# keychain. Keep a backup of it: a new certificate means every user grants access again.
identity=$(security find-identity -p codesigning | sed -n 's/.*) \([0-9A-F]\{40\}\) "Stickmouse Developer".*/\1/p' | head -n 1)
if [ -z "$identity" ]; then
    echo "warning: no \"Stickmouse Developer\" certificate, signing ad hoc" >&2
    identity=-
fi
# Sparkle ships signed ad hoc. Sign its parts inside out, then the app, with the same identity.
framework="$app/Contents/Frameworks/Sparkle.framework/Versions/B"
for part in "$framework"/XPCServices/*.xpc "$framework/Autoupdate" "$framework/Updater.app" \
    "$app/Contents/Frameworks/Sparkle.framework" "$app"; do
    codesign --force --sign "$identity" "$part"
done
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

# The website serves the latest build twice: as Stickmouse.dmg for the download button, and
# under its version for Sparkle, so a cached old file never fails the update's signature check.
mkdir -p website/downloads
rm -f website/downloads/Stickmouse-*.dmg
cp "$dmg" website/downloads/Stickmouse.dmg
cp "$dmg" "website/downloads/Stickmouse-$version.dmg"
cp packaging/macos/THIRD_PARTY_LICENSES.html website/

# The feeds and changelog page are committed, unlike the downloads, so publishing the website
# announces the new version. sign_update prints: sparkle:edSignature="..." length="..."
signed=$("$sparkle/bin/sign_update" --account stickmouse "$dmg")
signature=$(echo "$signed" | sed -n 's/.*edSignature="\([^"]*\)".*/\1/p')
length=$(echo "$signed" | sed -n 's/.*length="\([0-9]*\)".*/\1/p')
python3 packaging/macos/release_notes.py "$version" "Stickmouse-$version.dmg" "$length" "$signature"
echo "Copied to website/ and wrote appcast.xml, version.json and changelog.html"
