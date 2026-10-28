#!/bin/sh
set -e

DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
BUILD_DIR="$ROOT/target/rpmbuild"

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

cp "$ROOT/target/release/lenvix" "$BUILD_DIR/SOURCES/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.desktop" "$BUILD_DIR/SOURCES/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.svg" "$BUILD_DIR/SOURCES/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.metainfo.xml" "$BUILD_DIR/SOURCES/"
cp "$DIR/lenvix.spec" "$BUILD_DIR/SPECS/"

rpmbuild --define "_topdir $BUILD_DIR" -bb "$BUILD_DIR/SPECS/lenvix.spec"

mkdir -p "$ROOT/dist"
cp "$BUILD_DIR"/RPMS/*/*.rpm "$ROOT/dist/"
echo "RPM built successfully in $ROOT/dist/!"
