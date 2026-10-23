#!/usr/bin/env bash
# Builds dist/Generals2-x86_64.AppImage: the client, built in a Debian 12 container (glibc 2.36,
# so it runs on Debian 12 / Ubuntu 22.10 and newer), plus the original Generals / Zero Hour
# multiplayer maps from EA's GPL map sources and their licence.
#
# Usage: scripts/build-appimage.sh            (maps included, ~45 MB extra)
#        scripts/build-appimage.sh --no-maps  (built-in Crater Ridge only)
#
# The AppImage loads X11, Vulkan and the GPU driver from the host; it bundles no system
# libraries. Needs docker and network (crates, appimagetool, the AppImage runtime).
set -euo pipefail
cd "$(dirname "$0")/.."

MAPS=1
[ "${1:-}" = "--no-maps" ] && MAPS=0

IMAGE=rust:1.99-slim-bookworm
TOOLS=target/appimage-tools
APPDIR=target/AppDir
OUT=dist/Generals2-x86_64.AppImage

# Rootless Docker maps container root to this user, so build output stays owned by us. The
# container gets its own target dir so it never mixes with host builds (different glibc).
docker run --rm \
    -v "$PWD:/src" -v "${CARGO_HOME:-$HOME/.cargo}/registry:/usr/local/cargo/registry" \
    -w /src -e CARGO_TARGET_DIR=/src/target/bookworm \
    "$IMAGE" bash -c 'apt-get update -qq && apt-get install -y -qq --no-install-recommends \
        pkg-config libx11-dev libxkbcommon-dev libasound2-dev libudev-dev >/dev/null \
        && cargo build --release --locked -p g2-client'

BIN=target/bookworm/release/g2-client
GLIBC=$(objdump -T "$BIN" | grep -oE 'GLIBC_[0-9.]+' | sort -uV | tail -n1)
echo "built $BIN (needs $GLIBC)"

mkdir -p "$TOOLS"
fetch() { [ -s "$TOOLS/$1" ] || curl -fsSL -o "$TOOLS/$1" "$2"; }
fetch appimagetool https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
fetch runtime https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64
chmod +x "$TOOLS/appimagetool"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/generals2"
install -m 755 "$BIN" "$APPDIR/usr/bin/g2-client"
strip "$APPDIR/usr/bin/g2-client"
cp README.md "$APPDIR/usr/share/generals2/"

if [ "$MAPS" = 1 ]; then
    SRC=vendor/ea/CnC_Modding_Support
    [ -d "$SRC/Zero Hour/Maps/official" ] || { echo "map sources missing: git submodule update --init $SRC" >&2; exit 1; }
    DST="$APPDIR/usr/share/generals2/maps"
    # Only the .map files the client reads, in the layout g2_maps::catalogue expects.
    for game in Generals "Zero Hour"; do
        (cd "$SRC/$game/Maps/official" && find . -name '*.map' -print0) | while IFS= read -r -d '' f; do
            mkdir -p "$DST/$game/Maps/official/$(dirname "$f")"
            cp "$SRC/$game/Maps/official/$f" "$DST/$game/Maps/official/$f"
        done
    done
    # EA's GPL-3.0 release with its section 7 additional terms must travel with the data.
    cp "$SRC/LICENSE.md" "$DST/LICENSE-EA.md"
fi

cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
# The client is X11-only; on a Wayland session it runs through XWayland via $DISPLAY.
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/g2-client" "$@"
EOF
chmod +x "$APPDIR/AppRun"

cat > "$APPDIR/generals2.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Generals 2 (open-source recreation)
Comment=Open-source RTS engine inspired by the cancelled Generals 2
Exec=g2-client
Icon=generals2
Categories=Game;StrategyGame;
Terminal=false
EOF

# Icon: a teal disc on gunmetal, drawn here so no artwork is needed.
python3 - "$APPDIR/generals2.png" <<'EOF'
import struct, sys, zlib
n = 256
rows = []
for y in range(n):
    row = bytearray([0])
    for x in range(n):
        d = ((x - n / 2) ** 2 + (y - n / 2) ** 2) ** 0.5
        row += bytes((46, 199, 219, 255)) if d < n * 0.36 else bytes((18, 20, 23, 255))
    rows.append(bytes(row))
def chunk(t, d):
    return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
EOF

mkdir -p dist
# appimagetool is itself an AppImage; extract-and-run avoids needing FUSE on the build host.
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOLS/appimagetool" --runtime-file "$TOOLS/runtime" \
    --no-appstream "$APPDIR" "$OUT" >/dev/null
ls -la "$OUT"
