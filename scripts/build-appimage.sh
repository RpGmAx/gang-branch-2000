#!/usr/bin/env bash
# Builds dist/gang-branch-2000-x86_64.AppImage. Needs appimagetool on PATH.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v appimagetool >/dev/null || {
  echo "appimagetool introuvable (paru -S appimagetool-bin)" >&2
  exit 1
}

cargo build --release

appdir=target/AppDir
rm -rf "$appdir"
install -Dm755 target/release/gang-branch-2000 "$appdir/usr/bin/gang-branch-2000"
install -Dm644 assets/gang-branch-2000.desktop "$appdir/gang-branch-2000.desktop"
install -Dm644 assets/gang-branch-2000.svg "$appdir/usr/share/icons/hicolor/scalable/apps/gangbranch2000.svg"
# appimagetool derives .DirIcon from this PNG; it rejects an SVG one.
rsvg-convert -w 256 -h 256 assets/gang-branch-2000.svg -o "$appdir/gangbranch2000.png"
ln -s usr/bin/gang-branch-2000 "$appdir/AppRun"

mkdir -p dist
# Build aside then rename: the previous AppImage may still be running.
ARCH=x86_64 appimagetool --no-appstream "$appdir" dist/gang-branch-2000-x86_64.AppImage.new
mv -f dist/gang-branch-2000-x86_64.AppImage.new dist/gang-branch-2000-x86_64.AppImage
echo "dist/gang-branch-2000-x86_64.AppImage"
