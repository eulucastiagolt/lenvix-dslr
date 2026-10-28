#!/bin/sh
set -e

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

echo "==> 1. Compilando binário de release..."
cargo build --release

echo "==> 2. Gerando pacote Debian (.deb)..."
./packaging/deb/build-deb.sh

echo "==> 3. Gerando pacote Fedora/RHEL (.rpm)..."
./packaging/rpm/build-rpm.sh

echo "==> 4. Gerando pacote Arch Linux (.pkg.tar.zst)..."
(
    cd packaging/arch
    makepkg -f
    zstd -19 -T0 --rm lenvix-bin-*.pkg.tar -o "$ROOT/dist/lenvix-bin-0.1.0-1-x86_64.pkg.tar.zst" 2>/dev/null || true
)

echo "==> 5. Gerando pacote portátil (.tar.gz)..."
mkdir -p dist/lenvix-v0.1.0-linux-x86_64/{bin,share/applications,share/icons/hicolor/scalable/apps}
cp target/release/lenvix dist/lenvix-v0.1.0-linux-x86_64/bin/
cp assets/io.github.eulucastiagolt.lenvix.desktop dist/lenvix-v0.1.0-linux-x86_64/share/applications/
cp assets/io.github.eulucastiagolt.lenvix.svg dist/lenvix-v0.1.0-linux-x86_64/share/icons/hicolor/scalable/apps/
cp README.md dist/lenvix-v0.1.0-linux-x86_64/
cat << 'EOF' > dist/lenvix-v0.1.0-linux-x86_64/install.sh
#!/bin/sh
set -e
PREFIX="${PREFIX:-/usr/local}"
echo "Installing Lenvix to $PREFIX..."
install -Dm755 bin/lenvix "$PREFIX/bin/lenvix"
install -Dm644 share/applications/io.github.eulucastiagolt.lenvix.desktop "$PREFIX/share/applications/io.github.eulucastiagolt.lenvix.desktop"
install -Dm644 share/icons/hicolor/scalable/apps/io.github.eulucastiagolt.lenvix.svg "$PREFIX/share/icons/hicolor/scalable/apps/io.github.eulucastiagolt.lenvix.svg"
echo "Lenvix installed successfully!"
EOF
chmod +x dist/lenvix-v0.1.0-linux-x86_64/install.sh
tar -czf dist/lenvix-v0.1.0-linux-x86_64.tar.gz -C dist lenvix-v0.1.0-linux-x86_64

echo "==> 6. Gerando AppImage (.AppImage)..."
./packaging/appimage/build-appimage.sh

echo ""
echo "🎉 Build concluída com sucesso! Arquivos gerados em dist/:"
ls -lh "$ROOT/dist"
