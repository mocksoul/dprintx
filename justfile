# Build debug binary
build:
    cargo build

# Build release binary
build-release:
    cargo build --release

# Install (symlink already points to target/release)
install: build-release

# Run all CI checks (test + clippy + fmt)
check:
    cargo test
    cargo clippy -- -D warnings
    cargo fmt --check
