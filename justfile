# Detect clippy/fmt: prefer cargo subcommand (rustup), fall back to standalone binary (Gentoo)
clippy := if `cargo clippy --version 2>/dev/null; echo $?` =~ "0$" { "cargo clippy" } else { "cargo-clippy --" }
fmt := if `cargo fmt --version 2>/dev/null; echo $?` =~ "0$" { "cargo fmt" } else { "cargo-fmt" }

# Build debug binary
build:
    cargo build

# Build release binary
build-release:
    cargo build --release

# Install (symlink already points to target/release)
install: build-release

# Run all CI checks (dprint fmt + clippy + cargo fmt + test)
check:
    which dprint >/dev/null 2>&1 && dprint fmt || true
    {{ clippy }} -- -D warnings
    {{ fmt }} --check
    cargo test
