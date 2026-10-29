#!/bin/sh
set -e

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

# Versão sempre vem do Cargo.toml — nunca hardcode (evita pacotes defasados)
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
[ -n "$VERSION" ] || { echo "ERRO: versão não encontrada no Cargo.toml"; exit 1; }
export LENVIX_VERSION="$VERSION"

echo "==> 0. Limpando dist/ (sem artefatos de versões antigas)..."
rm -rf "$ROOT/dist"
mkdir -p "$ROOT/dist"

echo "==> 1. Compilando binário de release..."
cargo build --release

echo "==> 2. Gerando pacote Debian (.deb)..."
./packaging/deb/build-deb.sh

echo "==> 3. Gerando pacote Fedora/RHEL (.rpm)..."
./packaging/rpm/build-rpm.sh

echo "==> 4. Gerando pacote Arch Linux (.pkg.tar.zst)..."
(
    cd packaging/arch
    rm -f lenvix-bin-*.pkg.tar*   # sem pacotes defasados de versões antigas
    makepkg -f
    if ls lenvix-bin-*.pkg.tar 1> /dev/null 2>&1; then
        zstd -f -19 -T0 --rm lenvix-bin-*.pkg.tar -o "$ROOT/dist/lenvix-bin-$VERSION-1-x86_64.pkg.tar.zst"
    fi
    if ls lenvix-bin-*.pkg.tar.zst 1> /dev/null 2>&1; then
        cp -f lenvix-bin-*.pkg.tar.zst "$ROOT/dist/"
    fi
)

echo "==> 5. Gerando pacote portátil (.tar.gz)..."
TARDIR="dist/lenvix-v$VERSION-linux-x86_64"
mkdir -p "$TARDIR/bin" "$TARDIR/share/applications" "$TARDIR/share/icons/hicolor/scalable/apps"
cp target/release/lenvix "$TARDIR/bin/"
cp assets/io.github.eulucastiagolt.lenvix.desktop "$TARDIR/share/applications/"
cp assets/io.github.eulucastiagolt.lenvix.svg "$TARDIR/share/icons/hicolor/scalable/apps/"
cp README.md "$TARDIR/"
cat << 'EOF' > "$TARDIR/install.sh"
#!/bin/sh
set -e
PREFIX="${PREFIX:-/usr/local}"
echo "Installing Lenvix to $PREFIX..."
install -Dm755 bin/lenvix "$PREFIX/bin/lenvix"
install -Dm644 share/applications/io.github.eulucastiagolt.lenvix.desktop "$PREFIX/share/applications/io.github.eulucastiagolt.lenvix.desktop"
install -Dm644 share/icons/hicolor/scalable/apps/io.github.eulucastiagolt.lenvix.svg "$PREFIX/share/icons/hicolor/scalable/apps/io.github.eulucastiagolt.lenvix.svg"
echo "Lenvix installed successfully!"
EOF
chmod +x "$TARDIR/install.sh"
tar -czf "dist/lenvix-v$VERSION-linux-x86_64.tar.gz" -C dist "lenvix-v$VERSION-linux-x86_64"
rm -rf "$TARDIR"

echo "==> 6. Gerando AppImage (.AppImage)..."
./packaging/appimage/build-appimage.sh

echo ""
echo "🎉 Build v$VERSION concluída com sucesso! Arquivos gerados em dist/:"
ls -lh "$ROOT/dist"
