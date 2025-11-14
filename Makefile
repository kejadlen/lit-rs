.PHONY: tangle build test lint clean

# Tangle literate source files to generate src/
tangle:
	cargo run --quiet -- lit .
	cargo fmt

# Build the project
build: tangle
	cargo build

# Run tests
test: tangle
	cargo test

# Run lints
lint: tangle
	cargo check
	cargo clippy

# Clean generated files and restore src/ from version control
clean:
	jj restore src
	cargo clean
