.PHONY: all build dev dist release clean lint test check

all: build

# Compilar binário release
build:
	./build.sh --binary-only

# Rodar em modo release
dev:
	cargo run --release

# Gerar todos os instaladores (AppImage, deb, rpm, pacman, tarball)
dist:
	./build.sh

VERSION ?= $(shell grep '^version = ' Cargo.toml | head -n1 | cut -d '"' -f2)

# Publicar instaladores no GitHub Releases via terminal (requer gh autenticado)
# Versões 0.x.x (alpha) são marcadas como pre-release automaticamente
PRERELEASE != if grep -q '^version = "0' Cargo.toml; then echo --prerelease; else echo ""; fi

release: dist
	gh release create v$(VERSION) dist/*.AppImage dist/*.deb dist/*.rpm dist/*.pkg.tar.zst dist/*.tar.gz dist/*.zsync --title "Lenvix v$(VERSION)" $(PRERELEASE) --generate-notes || gh release upload v$(VERSION) dist/*.AppImage dist/*.deb dist/*.rpm dist/*.pkg.tar.zst dist/*.tar.gz dist/*.zsync --clobber

lint:
	cargo clippy --all-targets

test:
	cargo test

check:
	cargo check

clean:
	cargo clean
	rm -rf dist target/deb-build target/rpmbuild target/AppDir
