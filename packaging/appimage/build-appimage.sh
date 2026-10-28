#!/bin/sh
set -e

DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
APP_DIR="$ROOT/target/AppDir"

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR"/usr/{bin,share/applications,share/icons/hicolor/scalable/apps,share/metainfo}

cp "$ROOT/target/release/lenvix" "$APP_DIR/usr/bin/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.desktop" "$APP_DIR/usr/share/applications/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.desktop" "$APP_DIR/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.svg" "$APP_DIR/usr/share/icons/hicolor/scalable/apps/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.svg" "$APP_DIR/io.github.eulucastiagolt.lenvix.svg"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.svg" "$APP_DIR/.DirIcon"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.metainfo.xml" "$APP_DIR/usr/share/metainfo/"

cat << 'EOF' > "$APP_DIR/AppRun"
#!/bin/sh
SELF=$(readlink -f "$0")
HERE=${SELF%/*}
export PATH="${HERE}/usr/bin:${PATH}"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH}"
export XDG_DATA_DIRS="${HERE}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "${HERE}/usr/bin/lenvix" "$@"
EOF
chmod +x "$APP_DIR/AppRun"

# Download appimagetool if not available locally
if ! command -v appimagetool >/dev/null 2>&1; then
    if [ ! -f /tmp/appimagetool ]; then
        echo "Downloading appimagetool..."
        curl -sLo /tmp/appimagetool "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
        chmod +x /tmp/appimagetool
    fi
    TOOL="/tmp/appimagetool"
else
    TOOL="appimagetool"
fi

mkdir -p "$ROOT/dist"
ARCH=x86_64 "$TOOL" --appimage-extract-and-run "$APP_DIR" "$ROOT/dist/Lenvix-0.1.0-x86_64.AppImage" || \
ARCH=x86_64 "$TOOL" "$APP_DIR" "$ROOT/dist/Lenvix-0.1.0-x86_64.AppImage"
echo "AppImage created in $ROOT/dist/!"
