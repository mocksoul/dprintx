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

# Run all CI checks (dprint check + clippy + cargo fmt + test)
check:
    which dprint >/dev/null 2>&1 && dprint check || true
    {{ clippy }} -- -D warnings
    {{ fmt }} --check
    cargo test

# Release: bump version, commit, tag, push (e.g. just release 0.2.0)
release version:
    #!/usr/bin/env bash
    set -euo pipefail
    # Validate semver format
    if ! echo "{{ version }}" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        echo "error: invalid semver '{{ version }}', expected X.Y.Z" >&2
        exit 1
    fi
    # Check clean working tree
    if [ -n "$(git status --porcelain)" ]; then
        echo "error: working tree is dirty, commit or stash first" >&2
        exit 1
    fi
    # Bump version in Cargo.toml
    sed -i 's/^version = ".*"/version = "{{ version }}"/' Cargo.toml
    cargo generate-lockfile
    # Commit, tag, push
    git add Cargo.toml Cargo.lock
    git commit -m "release: v{{ version }}"
    git tag "v{{ version }}"
    git push && git push --tags
    echo "released v{{ version }}"
