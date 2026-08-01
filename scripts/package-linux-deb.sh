#!/usr/bin/env bash
# Build a simple .deb that installs mouse-suite to /usr/local.
# Usage: package-linux-deb.sh <binary> <version> [outdir]
set -euo pipefail

BINARY="${1:?binary path required}"
VERSION="${2:?version required}"
OUTDIR="${3:-dist}"
VERSION="${VERSION#v}"
ARCH="amd64"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG_ROOT="$OUTDIR/deb-root"
DEB_NAME="mouse-suite_${VERSION}_${ARCH}.deb"

[[ -f "$BINARY" ]] || { echo "missing binary: $BINARY" >&2; exit 1; }

rm -rf "$PKG_ROOT"
mkdir -p "$PKG_ROOT/DEBIAN"
mkdir -p "$PKG_ROOT/usr/local/bin"
mkdir -p "$PKG_ROOT/usr/local/share/mouse-suite"
mkdir -p "$PKG_ROOT/usr/share/applications"
mkdir -p "$PKG_ROOT/usr/share/doc/mouse-suite"

install -m 755 "$BINARY" "$PKG_ROOT/usr/local/bin/mouse-suite"
cp "$ROOT/config.toml" "$PKG_ROOT/usr/local/share/mouse-suite/" 2>/dev/null || true
cp "$ROOT/README.md" "$PKG_ROOT/usr/share/doc/mouse-suite/" 2>/dev/null || true
cp "$ROOT/AGENT_BRIDGE.md" "$PKG_ROOT/usr/share/doc/mouse-suite/" 2>/dev/null || true
cp "$ROOT/LICENSE" "$PKG_ROOT/usr/share/doc/mouse-suite/copyright" 2>/dev/null || true
if [[ -d "$ROOT/workflows" ]]; then
  cp -R "$ROOT/workflows" "$PKG_ROOT/usr/local/share/mouse-suite/"
fi

cat > "$PKG_ROOT/usr/share/applications/mouse-suite.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Mouse Suite
Comment=Recorder, clicker, flow editor, and agent bridge
Exec=/usr/local/bin/mouse-suite
Terminal=false
Categories=Utility;Development;
EOF

SIZE_KB=$(du -sk "$PKG_ROOT" | cut -f1)
cat > "$PKG_ROOT/DEBIAN/control" <<EOF
Package: mouse-suite
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${SIZE_KB}
Maintainer: Mouse Suite <noreply@users.noreply.github.com>
Homepage: https://github.com/1716775457damn/mouse-suite
Description: Desktop automation suite (recorder, clicker, flow, agent bridge)
 Experimental Linux build. GUI available; click injection is stubbed.
EOF

mkdir -p "$OUTDIR"
dpkg-deb --build --root-owner-group "$PKG_ROOT" "$OUTDIR/$DEB_NAME"
echo "DEB: $OUTDIR/$DEB_NAME"
