.PHONY: all build dev dist release clean lint test check

all: build

# Compilar binário release
build:
	cargo build --release

# Rodar em modo release
dev:
	cargo run --release

# Gerar todos os instaladores (AppImage, deb, rpm, pacman, tarball)
dist:
	./build.sh

VERSION ?= $(shell grep '^version = ' Cargo.toml | head -n1 | cut -d '"' -f2)

# Publicar instaladores no GitHub Releases via terminal (requer gh autenticado)
release: dist
	gh release create v$(VERSION) dist/*.AppImage dist/*.deb dist/*.rpm dist/*.pkg.tar.zst dist/*.tar.gz --title "Lenvix v$(VERSION)" --generate-notes || gh release upload v$(VERSION) dist/*.AppImage dist/*.deb dist/*.rpm dist/*.pkg.tar.zst dist/*.tar.gz --clobber

lint:
	cargo clippy --all-targets

test:
	cargo test

check:
	cargo check

clean:
	cargo clean
	rm -rf dist target/deb-build target/rpmbuild target/AppDir
