set shell := ["bash", "-cu"]

default:
    @just --list

build:
    cargo build --features experimental-discord

release:
    cargo build --release --features experimental-discord

run *args:
    cargo run --features experimental-discord -- {{args}}

fmt:
    cargo fmt --all

lint:
    cargo clippy --all --benches --tests --examples --all-features

test:
    cargo test --all-features
    cargo test

check:
    just fmt
    just lint
    just test

logs:
    tail -f "${HOME}/Library/Application Support/disctui/disctui.log"

version:
    @grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/'

# Requires cargo-release: cargo install cargo-release
bump: bump-patch

# Requires cargo-release: cargo install cargo-release
bump-patch:
    cargo release patch --execute --no-publish

# Requires cargo-release: cargo install cargo-release
bump-minor:
    cargo release minor --execute --no-publish

# Requires cargo-release: cargo install cargo-release
bump-major:
    cargo release major --execute --no-publish

# Requires cargo-release: cargo install cargo-release
bump-dry level="patch":
    cargo release {{level}}

tag version:
    @echo "Creating tag v{{version}}..."
    git tag -a "v{{version}}" -m "Release v{{version}}"
    git push origin "v{{version}}"

dist-generate:
    cargo dist generate

dist-plan:
    cargo dist plan

dist-build:
    cargo dist build --artifacts=local
