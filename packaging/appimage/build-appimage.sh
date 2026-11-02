#!/bin/bash
set -e

DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
VERSION="${LENVIX_VERSION:-0.4.0}"
APP_DIR="$ROOT/target/AppDir"

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/usr/bin" \
         "$APP_DIR/usr/share/applications" \
         "$APP_DIR/usr/share/icons/hicolor/scalable/apps" \
         "$APP_DIR/usr/share/icons/hicolor/256x256/apps" \
         "$APP_DIR/usr/share/metainfo" \
         "$APP_DIR/usr/share/appdata" \
         "$APP_DIR/usr/share/lenvix/i18n"

cp "$ROOT/target/release/lenvix" "$APP_DIR/usr/bin/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.desktop" "$APP_DIR/usr/share/applications/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.desktop" "$APP_DIR/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.svg" "$APP_DIR/usr/share/icons/hicolor/scalable/apps/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.svg" "$APP_DIR/io.github.eulucastiagolt.lenvix.svg"
if [ -f "$ROOT/assets/io.github.eulucastiagolt.lenvix.png" ]; then
    cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.png" "$APP_DIR/io.github.eulucastiagolt.lenvix.png"
    cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.png" "$APP_DIR/usr/share/icons/hicolor/256x256/apps/"
    cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.png" "$APP_DIR/.DirIcon"
fi
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.metainfo.xml" "$APP_DIR/usr/share/metainfo/"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.metainfo.xml" "$APP_DIR/usr/share/metainfo/io.github.eulucastiagolt.lenvix.appdata.xml"
cp "$ROOT/assets/io.github.eulucastiagolt.lenvix.metainfo.xml" "$APP_DIR/usr/share/appdata/io.github.eulucastiagolt.lenvix.appdata.xml"
if [ -d "$ROOT/i18n" ]; then
    cp -r "$ROOT/i18n/"* "$APP_DIR/usr/share/lenvix/i18n/"
fi

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

# update-information embutida no AppImage (seção .upd_info do ELF) — é o que
# os plugins de update (AppImageUpdate, AppManager) leem para autoatualizar.
# Aponta para o asset Lenvix-*.AppImage.zsync do release "latest" no GitHub.
UPDATE_INFO="gh-releases-zsync|eulucastiagolt/lenvix-dslr|latest|Lenvix-*.x86_64.AppImage.zsync"
APPIMG="$ROOT/dist/Lenvix-$VERSION-x86_64.AppImage"

ARCH=x86_64 "$TOOL" --updateinformation "$UPDATE_INFO" "$APP_DIR" "$APPIMG"

# Gera o .zsync ao lado do AppImage (asset exigido pelo gh-releases-zsync).
# zsyncmake vem embutido no próprio appimagetool (--appimage-extract).
(
    cd "$ROOT/target"
    rm -rf squashfs-root
    ARCH=x86_64 "$TOOL" --appimage-extract >/dev/null 2>&1 || true
)
ZSYNCMAKE="$ROOT/target/squashfs-root/usr/bin/zsyncmake"
if [ -x "$ZSYNCMAKE" ]; then
    (
        cd "$ROOT/dist"
        "$ZSYNCMAKE" -u "https://github.com/eulucastiagolt/lenvix-dslr/releases/download/v$VERSION/$(basename "$APPIMG")" "$(basename "$APPIMG")" 2>/dev/null
    )
fi
rm -rf "$ROOT/target/squashfs-root"
echo "AppImage + .zsync criados em $ROOT/dist/!"
