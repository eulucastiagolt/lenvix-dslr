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

# Publicar instaladores no GitHub Releases via terminal (requer gh autenticado)
release: dist
	gh release create v0.1.0 dist/* --title "Lenvix v0.1.0" --generate-notes || gh release upload v0.1.0 dist/* --clobber

lint:
	cargo clippy --all-targets

test:
	cargo test

check:
	cargo check

clean:
	cargo clean
	rm -rf dist target/deb-build target/rpmbuild target/AppDir
