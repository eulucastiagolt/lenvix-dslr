.PHONY: all build dev dist clean lint test check

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

lint:
	cargo clippy --all-targets

test:
	cargo test

check:
	cargo check

clean:
	cargo clean
	rm -rf dist target/deb-build target/rpmbuild target/AppDir
