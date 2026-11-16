#!/usr/bin/env bash
# Copies the AppImage to ~/Applications. Its first launch adds it to the app menu.
set -euo pipefail
cd "$(dirname "$0")/.."

target="$HOME/Applications/gang-branch-2000.AppImage"
install -Dm755 dist/gang-branch-2000-x86_64.AppImage "$target"
echo "Installé : $target (lancez-le une fois pour l'ajouter au menu)"
