# dprint-mconf

A wrapper around [dprint](https://dprint.dev/) that adds per-file config profiles.

## Why?

dprint doesn't support per-file config overrides ([#996](https://github.com/dprint/dprint/issues/996)).
This wrapper selects the right dprint config based on file path using glob rules.

## How it works

Config file: `~/.config/dprint/mconf.jsonc`

```jsonc
{
  "dprint": "~/.cargo/bin/dprint",
  "profiles": {
    "maintainer": "~/.config/dprint/dprint-maintainer.jsonc",
    "default": "~/.config/dprint/dprint-default.jsonc"
  },
  "match": {
    "**/noc/cmdb/**": "maintainer",
    "**/noc/invapi/**": "maintainer",
    "**": "default"
  },
  "diff_pager": "delta -s"
}
```

Rules in `match` are evaluated top-to-bottom, first match wins. Files not matching any rule are skipped. Use `"**": "profile"` as a catch-all.

### diff_pager

When `diff_pager` is set, `dprint check` produces unified diff output instead of dprint's default format:

- **stdout is TTY** → pipes through the pager (e.g. `delta -s`)
- **stdout is pipe/redirect** → raw unified diff

```bash
dprint check              # pretty diff via delta
dprint check > fix.patch  # unified diff to file
```

Without `diff_pager`, `dprint check` behaves exactly like the original dprint.

## CLI

```bash
# stdin — single file, filename is used for config matching (input is read from stdin)
dprint fmt --stdin path/to/file.yaml < input.yaml

# fmt/check — groups files by profile, calls dprint per group
dprint fmt
dprint check
dprint fmt file1.go file2.yaml   # explicit file list

# list all files that would be formatted (merged from all profiles)
dprint output-file-paths

# show which config is used
dprint config              # all profiles and rules
dprint config path/to/file # resolved config for a file

# LSP proxy — spawns dprint lsp per profile, routes by file URI
dprint lsp
```

`dprint check` exits with code 1 if any files need formatting.

Use `--mconf <PATH>` to override the config location (default: `~/.config/dprint/mconf.jsonc`):

```bash
dprint --mconf /path/to/custom.jsonc fmt
```

All unknown commands and flags are passed through to the real dprint (`--help`, `-V`, `license`, `completions`, etc.).

## Install

```bash
cargo build --release
ln -sf $(pwd)/target/release/dprint-mconf ~/.local/bin/dprint
```
