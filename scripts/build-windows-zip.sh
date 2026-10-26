#!/usr/bin/env bash
# Builds dist/Generals2-windows-x86_64.zip on Windows (Git Bash): the client, the original
# Generals / Zero Hour maps in maps\ beside it and their licence. Unzip anywhere and run
# g2-client.exe. Not code-signed: SmartScreen asks once (More info, Run anyway).
#
# Usage: scripts/build-windows-zip.sh   (needs the MSVC Rust toolchain and 7-Zip)
set -euo pipefail
cd "$(dirname "$0")/.."

DIR="target/windows/Open RTS"
OUT=dist/Generals2-windows-x86_64.zip

cargo build --release --locked -p g2-client
rm -rf "$DIR"
mkdir -p "$DIR"
cp target/release/g2-client.exe "$DIR/"
scripts/copy-maps.sh "$DIR/maps"
cp README.md "$DIR/"

mkdir -p dist
rm -f "$OUT"
(cd target/windows && 7z a -tzip -mx=9 "../../$OUT" "Open RTS" >/dev/null)
ls -la "$OUT"
