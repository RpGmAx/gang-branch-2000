#!/usr/bin/env bash
# Builds dist/gang-branch-2000-macos.dmg, a universal app (Apple Silicon and Intel).
# Runs on macOS only; from Linux, use the GitHub workflow .github/workflows/package.yml.
set -euo pipefail
cd "$(dirname "$0")/.."

[ "$(uname -s)" = Darwin ] || {
  echo "À lancer sur un Mac, ou via la CI GitHub (.github/workflows/package.yml)" >&2
  exit 1
}

targets=(aarch64-apple-darwin x86_64-apple-darwin)
rustup target add "${targets[@]}"
for target in "${targets[@]}"; do
  cargo build --release --target "$target"
done

app="target/macos/Gang Branch 2000.app"
rm -rf target/macos
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/gang-branch-2000" \
  target/aarch64-apple-darwin/release/gang-branch-2000 target/x86_64-apple-darwin/release/gang-branch-2000
cp assets/icons/gangbranch2000.icns "$app/Contents/Resources/"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Gang Branch 2000</string>
  <key>CFBundleDisplayName</key><string>Gang Branch 2000</string>
  <key>CFBundleIdentifier</key><string>app.gangbranch2000</string>
  <key>CFBundleExecutable</key><string>gang-branch-2000</string>
  <key>CFBundleIconFile</key><string>gangbranch2000</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# Ad-hoc signature: Apple Silicon refuses to run an unsigned binary. Not notarized, so the
# first launch needs right-click > Open.
codesign --force --deep --sign - "$app"

staging=target/macos/dmg
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
mkdir -p dist
hdiutil create -volname "Gang Branch 2000" -srcfolder "$staging" -ov -format UDZO \
  dist/gang-branch-2000-macos.dmg
echo "dist/gang-branch-2000-macos.dmg"
