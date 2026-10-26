#!/usr/bin/env bash
# Builds dist/Generals2-macOS.dmg on a Mac: Open RTS.app, a universal (Apple silicon and
# Intel) build of the client with the original Generals / Zero Hour maps and their licence in
# its Resources. The app isn't signed or notarized: the first start needs right click, Open.
#
# Usage: scripts/build-macos-app.sh   (needs Xcode's command line tools and rustup)
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(git describe --tags --always)
APP="target/macos/Open RTS.app"
OUT=dist/Generals2-macOS.dmg

rustup target add aarch64-apple-darwin x86_64-apple-darwin
for t in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked -p g2-client --target "$t"
done

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/g2-client" \
    target/aarch64-apple-darwin/release/g2-client target/x86_64-apple-darwin/release/g2-client
strip "$APP/Contents/MacOS/g2-client"
scripts/copy-maps.sh "$APP/Contents/Resources/maps"
cp README.md "$APP/Contents/Resources/"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Open RTS</string>
  <key>CFBundleDisplayName</key><string>Open RTS</string>
  <key>CFBundleIdentifier</key><string>io.github.boredland.generals2</string>
  <key>CFBundleExecutable</key><string>g2-client</string>
  <key>CFBundleIconFile</key><string>generals2</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION#v}</string>
  <key>CFBundleVersion</key><string>${VERSION#v}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.strategy-games</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

# Icon: the AppImage's teal disc on gunmetal, drawn here so no artwork is needed.
ICONSET=target/macos/generals2.iconset
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
python3 - "$ICONSET" <<'EOF'
import struct, sys, zlib
def png(n):
    rows = []
    for y in range(n):
        row = bytearray([0])
        for x in range(n):
            d = ((x - n / 2) ** 2 + (y - n / 2) ** 2) ** 0.5
            row += bytes((46, 199, 219, 255)) if d < n * 0.36 else bytes((18, 20, 23, 255))
        rows.append(bytes(row))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b""))
for n in (16, 32, 128, 256, 512):
    open(f"{sys.argv[1]}/icon_{n}x{n}.png", "wb").write(png(n))
    open(f"{sys.argv[1]}/icon_{n // 2}x{n // 2}@2x.png", "wb").write(png(n)) if n >= 32 else None
EOF
iconutil -c icns -o "$APP/Contents/Resources/generals2.icns" "$ICONSET"

# Ad-hoc signed: Apple silicon refuses to run unsigned code at all.
codesign --force --deep --sign - "$APP"

mkdir -p dist
rm -f "$OUT"
hdiutil create -volname "Open RTS" -srcfolder "$APP" -ov -format UDZO "$OUT"
ls -la "$OUT"
