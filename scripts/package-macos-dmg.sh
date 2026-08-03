#!/usr/bin/env bash
# Build Mouse Suite.app and a drag-to-Applications DMG.
# Usage: package-macos-dmg.sh <binary> <version> <arch> [outdir]
set -euo pipefail

BINARY="${1:?binary path required}"
VERSION="${2:?version required}"
ARCH="${3:?arch required (x86_64|aarch64)}"
OUTDIR="${4:-dist}"
VERSION="${VERSION#v}"

APP_NAME="Mouse Suite"
BUNDLE_ID="com.mousesuite.app"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STAGE="$OUTDIR/dmg-stage-$ARCH"
APP="$STAGE/$APP_NAME.app"
DMG_NAME="Mouse-Suite-${VERSION}-macos-${ARCH}.dmg"
DMG_PATH="$OUTDIR/$DMG_NAME"

[[ -f "$BINARY" ]] || { echo "missing binary: $BINARY" >&2; exit 1; }

rm -rf "$STAGE"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "$BINARY" "$APP/Contents/MacOS/mouse-suite"
chmod +x "$APP/Contents/MacOS/mouse-suite"

sed "s/VERSION_PLACEHOLDER/${VERSION}/g" \
  "$ROOT/packaging/macos/Info.plist" > "$APP/Contents/Info.plist"

# Ship docs next to the app for first-run reference
mkdir -p "$STAGE/Docs"
cp "$ROOT/config.toml" "$STAGE/Docs/" 2>/dev/null || true
cp "$ROOT/README.md" "$STAGE/Docs/" 2>/dev/null || true
cp "$ROOT/AGENT_BRIDGE.md" "$STAGE/Docs/" 2>/dev/null || true
if [[ -d "$ROOT/workflows" ]]; then
  cp -R "$ROOT/workflows" "$STAGE/Docs/"
fi

# Ad-hoc sign so Gatekeeper at least sees a signature (not notarized)
# Build AppIcon.icns from source PNG when available
ICON_SRC="$ROOT/packaging/macos/AppIcon-1024.png"
if [[ -f "$ICON_SRC" ]]; then
  ICONSET="$STAGE/AppIcon.iconset"
  mkdir -p "$ICONSET"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
  rm -rf "$ICONSET"
elif [[ -f "$ROOT/packaging/macos/AppIcon.icns" ]]; then
  cp "$ROOT/packaging/macos/AppIcon.icns" "$APP/Contents/Resources/"
fi

# Default config into Resources (read-only); runtime writes to Application Support
if [[ -f "$ROOT/config.toml" ]]; then
  cp "$ROOT/config.toml" "$APP/Contents/Resources/config.toml"
fi

codesign --force --deep --sign - "$APP" || true

ln -sf /Applications "$STAGE/Applications"

rm -f "$DMG_PATH"
hdiutil create \
  -volname "Mouse Suite ${VERSION}" \
  -srcfolder "$STAGE" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

# Also keep a zipped .app for users who prefer not to use DMG
mkdir -p "$OUTDIR"
APP_ZIP="$(cd "$OUTDIR" && pwd)/Mouse-Suite-${VERSION}-macos-${ARCH}.app.zip"
rm -f "$APP_ZIP"
(
  cd "$STAGE"
  zip -qry "$APP_ZIP" "$APP_NAME.app"
)

echo "DMG: $DMG_PATH"
echo "APP ZIP: $APP_ZIP"
