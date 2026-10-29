#!/bin/sh
set -e

DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
BUILD_DIR="$ROOT/target/deb-build"
VERSION="${LENVIX_VERSION:-0.3.0}"
DEB_NAME="lenvix_${VERSION}_amd64.deb"

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/data/usr/bin"
mkdir -p "$BUILD_DIR/data/usr/share/applications"
mkdir -p "$BUILD_DIR/data/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$BUILD_DIR/data/usr/share/metainfo"
mkdir -p "$BUILD_DIR/control"

# Copy files
install -m755 "$ROOT/target/release/lenvix" "$BUILD_DIR/data/usr/bin/lenvix"
install -m644 "$ROOT/assets/io.github.eulucastiagolt.lenvix.desktop" "$BUILD_DIR/data/usr/share/applications/"
install -m644 "$ROOT/assets/io.github.eulucastiagolt.lenvix.svg" "$BUILD_DIR/data/usr/share/icons/hicolor/scalable/apps/"
install -m644 "$ROOT/assets/io.github.eulucastiagolt.lenvix.metainfo.xml" "$BUILD_DIR/data/usr/share/metainfo/"

# Create control file (EOF sem aspas: versão vem da variável)
cat << EOF > "$BUILD_DIR/control/control"
Package: lenvix
Version: $VERSION
Section: video
Priority: optional
Architecture: amd64
Depends: libgtk-4-1, libgphoto2-6
Recommends: v4l2loopback-dkms
Maintainer: Lucas Tiago <lucastiagolt>
Description: DSLR and mirrorless live preview, controls and virtual webcam for Linux
 Lenvix turns your DSLR or mirrorless camera into a studio-grade virtual
 webcam on Linux using v4l2loopback and libgphoto2, with full camera controls
 and dynamic collision avoidance.
EOF

# Debian binary spec
echo "2.0" > "$BUILD_DIR/debian-binary"

# Create archives
tar --owner=0 --group=0 -czf "$BUILD_DIR/control.tar.gz" -C "$BUILD_DIR/control" .
tar --owner=0 --group=0 -czf "$BUILD_DIR/data.tar.gz" -C "$BUILD_DIR/data" .

# Package into .deb using ar
mkdir -p "$ROOT/dist"
cd "$BUILD_DIR"
ar -rc "$ROOT/dist/$DEB_NAME" debian-binary control.tar.gz data.tar.gz

echo "Created $ROOT/dist/$DEB_NAME successfully!"
