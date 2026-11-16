#!/usr/bin/env bash
# Builds dist/gang-branch-2000-x86_64.exe: natively on Windows (Git Bash), or from Linux
# with mingw-w64 (sudo pacman -S mingw-w64-gcc; rustup target add x86_64-pc-windows-gnu).
set -euo pipefail
cd "$(dirname "$0")/.."

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    cargo build --release
    exe=target/release/gang-branch-2000.exe
    ;;
  *)
    target=x86_64-pc-windows-gnu
    rustup target list --installed | grep -qx "$target" || {
      echo "Cible Rust absente : rustup target add $target" >&2
      exit 1
    }
    command -v x86_64-w64-mingw32-gcc >/dev/null || {
      echo "mingw-w64 introuvable : sudo pacman -S mingw-w64-gcc" >&2
      exit 1
    }
    cargo build --release --target "$target"
    exe="target/$target/release/gang-branch-2000.exe"
    ;;
esac

mkdir -p dist
cp "$exe" dist/gang-branch-2000-x86_64.exe
echo "dist/gang-branch-2000-x86_64.exe"
