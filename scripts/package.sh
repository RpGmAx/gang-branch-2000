#!/usr/bin/env bash
# Usage: scripts/package.sh [linux|windows|macos|all]   (default: the current system)
# Everything lands in dist/. « all » builds what this machine can and says what it skipped.
set -euo pipefail
here="$(dirname "$0")"

case "$(uname -s)" in
  Darwin) current=macos ;;
  MINGW* | MSYS* | CYGWIN*) current=windows ;;
  *) current=linux ;;
esac

build() {
  case "$1" in
    linux) "$here/build-appimage.sh" ;;
    windows) "$here/build-windows.sh" ;;
    macos) "$here/build-macos.sh" ;;
    *) echo "Cible inconnue : $1 (linux, windows, macos ou all)" >&2; exit 1 ;;
  esac
}

if [ "${1:-$current}" != all ]; then
  build "${1:-$current}"
  exit
fi
skipped=()
for target in linux windows macos; do
  build "$target" || skipped+=("$target")
done
if [ ${#skipped[@]} -gt 0 ]; then
  echo "Non construit ici : ${skipped[*]} (la CI GitHub construit les trois)" >&2
fi
