.PHONY: build run test test-packaging lint fmt fmt-check package pacman app install clean

build:
	cargo build --locked -p aether

run:
	cargo run --locked -p aether

# hackernews::client tests flake on macOS and abort the whole test binary
test:
	cargo test --locked --workspace -- --skip hackernews::client

test-packaging:
	uv run -m unittest discover -s scripts/tests -v

lint:
	cargo clippy --locked --workspace --all-targets

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

package:
	uv run scripts/build.py

pacman:
	makepkg -f

app:
	uv run scripts/build.py --format app

install: app
	rm -rf /Applications/Aether.app
	ditto dist/Aether.app /Applications/Aether.app

clean:
	cargo clean
