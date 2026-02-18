# Build debug binary
build:
    cargo build

# Build release binary
build-release:
    cargo build --release

# Install (symlink already points to target/release)
install: build-release

# Run all CI checks (clippy + fmt + test)
check:
    cargo-clippy -- -- -D warnings
    cargo-fmt --check
    cargo test
