#!/usr/bin/env bash
# Regenerates every icon from assets/gang-branch-2000.svg (needs rsvg-convert, magick, python3).
set -euo pipefail
cd "$(dirname "$0")/.."

svg=assets/gang-branch-2000.svg
out=assets/icons
name=gangbranch2000
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for size in 16 32 48 64 128 256 512 1024; do
  rsvg-convert -w "$size" -h "$size" "$svg" -o "$tmp/$size.png"
done
mkdir -p "$out"
for size in 48 128 256; do cp "$tmp/$size.png" "$out/$name-$size.png"; done   # Linux hicolor
magick "$tmp"/{16,32,48,64,128,256}.png "$out/$name.ico"                       # Windows
magick "$tmp/128.png" "RGBA:$out/$name-128.rgba"                               # window icon (winit)

# macOS: an icns file is a header then PNGs, each tagged by its pixel size.
python3 - "$tmp" "$out/$name.icns" <<'PY'
import struct, sys
tmp, target = sys.argv[1], sys.argv[2]
types = [("icp4", 16), ("icp5", 32), ("ic11", 32), ("icp6", 64), ("ic12", 64), ("ic07", 128),
         ("ic08", 256), ("ic13", 256), ("ic09", 512), ("ic14", 512), ("ic10", 1024)]
body = b""
for tag, size in types:
    png = open(f"{tmp}/{size}.png", "rb").read()
    body += tag.encode() + struct.pack(">I", len(png) + 8) + png
open(target, "wb").write(b"icns" + struct.pack(">I", len(body) + 8) + body)
PY
echo "Icônes régénérées dans $out"
