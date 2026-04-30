default: check

setup:
    git config core.hooksPath .githooks

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

build:
    cargo build --workspace --locked

test:
    cargo test --workspace --locked

check: fmt-check clippy

clean:
    cargo clean

release tag:
    git tag {{tag}}
    git push origin {{tag}}
