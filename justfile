# Build debug binary
build:
    cargo build

# Build release binary
build-release:
    cargo build --release

# Install (symlink already points to target/release)
install: build-release
